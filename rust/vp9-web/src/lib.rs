//! The page's VP9 decoder, as zDesk's decode worker drives it: one frame
//! in, at most one picture out, the picture's planes left in this module's
//! memory for the paint worker to upload to the GPU from there.
//!
//! A frame's tiles decode, and its rows are filtered, on rayon's pool, and a page has no
//! threads to spawn: a thread is a worker the page starts, running an instance
//! of this module on the one memory. So the pool's threads are seats the page's
//! workers take ([`run_pool_thread`]) before the pool is made of them
//! ([`start_pool`]). A decoder made with one thread decodes on the thread that
//! calls it, and needs no pool.

use std::io;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use rayon::{ThreadBuilder, ThreadPoolBuilder};
use wasm_bindgen::prelude::*;

/// The pool's threads on their way from the pool that makes them to the workers
/// that run them.
struct Seats {
    offer: Mutex<Sender<ThreadBuilder>>,
    take: Mutex<Receiver<ThreadBuilder>>,
}

fn seats() -> &'static Seats {
    static SEATS: OnceLock<Seats> = OnceLock::new();
    SEATS.get_or_init(|| {
        let (offer, take) = channel();
        Seats { offer: Mutex::new(offer), take: Mutex::new(take) }
    })
}

/// The compiled module, for a worker to make its instance of.
#[wasm_bindgen]
pub fn module() -> JsValue {
    wasm_bindgen::module()
}

/// Run one of the pool's threads on the worker that calls this, which waits for
/// the pool to be started and returns when the pool is gone: never, in a page.
#[wasm_bindgen(js_name = runPoolThread)]
pub fn run_pool_thread() {
    let thread = seats().take.lock().expect("no thread panics holding a seat").recv();
    if let Ok(thread) = thread {
        thread.run();
    }
}

/// Make the pool, of `threads` workers that are each in [`run_pool_thread`]
/// already: this waits for every thread to have started, and a worker cannot
/// start while the thread that made it waits.
#[wasm_bindgen(js_name = startPool)]
pub fn start_pool(threads: usize) -> Result<(), JsError> {
    let offer = seats().offer.lock().expect("no thread panics holding a seat").clone();
    ThreadPoolBuilder::new()
        .num_threads(threads)
        .spawn_handler(move |thread| offer.send(thread).map_err(|_| io::Error::other("the pool's seats are gone")))
        .build_global()
        .map_err(|e| JsError::new(&e.to_string()))
}

/// How many numbers [`Decoder::picture`] describes a picture in.
const PICTURE_FIELDS: usize = 16;

/// What the page knows a picture by: its frame's serial in the decoder's
/// count, which counts from 1 and no frame of it shares, wrapped short of 0 to
/// fit 32 bits.
const WRAP: u64 = u32::MAX as u64;

fn page_serial(serial: u64) -> u32 {
    ((serial - 1) % WRAP) as u32 + 1
}

/// One stream's decoder.
#[wasm_bindgen]
pub struct Decoder {
    inner: vp9::Decoder,
    /// Where the page writes the next frame.
    input: Vec<u8>,
    /// The pictures the page is reading, by serial, each held until the page
    /// releases it: the next frames decode into other buffers meanwhile. The
    /// last is the one the last frame decoded to.
    held: Vec<(u32, vp9::Decoded)>,
    /// The last picture's serial.
    serial: u32,
    picture: [i32; PICTURE_FIELDS],
    changed: [i32; 4],
}

#[wasm_bindgen]
impl Decoder {
    /// A decoder whose frames decode on `threads` of the pool, or on the
    /// caller for one.
    #[wasm_bindgen(constructor)]
    pub fn new(threads: usize) -> Decoder {
        Decoder { inner: vp9::Decoder::new(threads), input: Vec::new(), held: Vec::new(), serial: 0, picture: [0; PICTURE_FIELDS], changed: [0; 4] }
    }

    /// Room for a frame of `size` bytes in this module's memory, where the
    /// page writes it before [`Self::decode`]. Good until that call. Throws
    /// for a size the memory has no room for.
    pub fn input(&mut self, size: usize) -> Result<*mut u8, JsError> {
        self.input.clear();
        self.input.try_reserve_exact(size).map_err(|_| JsError::new(&format!("no room for a frame of {size} bytes")))?;
        self.input.resize(size, 0);
        Ok(self.input.as_mut_ptr())
    }

    /// Decode the frame written into [`Self::input`]: true when it is one
    /// that is shown, which [`Self::picture`] describes and which is held
    /// until [`Self::release`]. Throws for a frame that does not decode,
    /// after which the stream waits for a keyframe.
    pub fn decode(&mut self) -> Result<bool, JsError> {
        let frame = std::mem::take(&mut self.input);
        let r = self.inner.decode(&frame);
        self.input = frame;
        let Some(decoded) = r.map_err(|e| JsError::new(&e.to_string()))? else {
            return Ok(false);
        };
        self.serial = page_serial(decoded.frame.serial());
        self.held.push((self.serial, decoded));
        Ok(true)
    }

    /// The page is done reading picture `serial`, whose buffers the decoder
    /// may have back. A serial not held is nothing.
    pub fn release(&mut self, serial: u32) {
        self.held.retain(|(s, _)| *s != serial);
    }

    /// The picture the last frame decoded to, in sixteen numbers, as
    /// hevc-wasm's module describes one: its width and height, `2` for 4:4:4,
    /// the colour range (`2` full, `1` limited), the matrix, primaries and
    /// transfer as ITU-T H.273 numbers them (`2` for unstated), each plane's
    /// start in this module's memory, each plane's stride, whether it is a
    /// keyframe, and its serial, which [`Self::release`] takes. The planes
    /// are good until it is released, and all sixteen are 0 once it is.
    pub fn picture(&mut self) -> *const i32 {
        let p = &mut self.picture;
        *p = [0; PICTURE_FIELDS];
        if let Some((serial, d)) = self.held.last().filter(|(s, _)| *s == self.serial) {
            let f = &d.frame;
            // VP9's color_space as H.273's matrix, primaries and transfer.
            let [matrix, primaries, transfer] = match f.colour.space {
                1 | 3 => [6, 6, 6],
                2 => [1, 1, 1],
                4 => [7, 7, 7],
                5 => [9, 9, 14],
                7 => [0, 1, 13],
                _ => [2, 2, 2],
            };
            p[0] = f.width as i32;
            p[1] = f.height as i32;
            p[2] = 2;
            p[3] = if f.colour.full_range { 2 } else { 1 };
            p[4] = matrix;
            p[5] = primaries;
            p[6] = transfer;
            for (i, plane) in f.planes.iter().enumerate() {
                p[7 + i] = plane.data.as_ptr() as i32;
                p[10 + i] = plane.stride as i32;
            }
            p[13] = d.keyframe as i32;
            p[14] = *serial as i32;
        }
        p.as_ptr()
    }

    /// Where the picture the last frame decoded to may differ from picture
    /// `since`, by its serial, in four numbers: the left, top, width and
    /// height in samples of a rectangle of the planes, the whole picture for
    /// a keyframe or a picture that does not come down from that one, and a
    /// width of 0 where nothing changed. A page that has that picture in its
    /// textures need upload only that. All four are 0 once the picture is
    /// released.
    pub fn changed(&mut self, since: u32) -> *const i32 {
        let c = &mut self.changed;
        *c = [0; 4];
        if let Some((_, d)) = self.held.last().filter(|(s, _)| *s == self.serial) {
            // The serial `since` is, counted back from this picture's.
            let serial = d.frame.serial();
            let back = (u64::from(self.serial) + WRAP - u64::from(since)) % WRAP;
            let rect = match since {
                1.. if back < serial => d.frame.changed_since(serial - back),
                _ => [0, 0, d.frame.width, d.frame.height],
            };
            *c = rect.map(|n| n as i32);
        }
        c.as_ptr()
    }
}

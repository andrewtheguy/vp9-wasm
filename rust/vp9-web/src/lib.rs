//! The page's VP9 decoder, as remotex's decode worker drives it: one frame
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

/// One stream's decoder.
#[wasm_bindgen]
pub struct Decoder {
    inner: vp9::Decoder,
    /// Where the page writes the next frame.
    input: Vec<u8>,
    /// The picture the last frame decoded to, held until the next frame so the
    /// page can read its planes.
    held: Option<vp9::Decoded>,
    picture: [i32; PICTURE_FIELDS],
}

#[wasm_bindgen]
impl Decoder {
    /// A decoder whose frames decode on `threads` of the pool, or on the
    /// caller for one.
    #[wasm_bindgen(constructor)]
    pub fn new(threads: usize) -> Decoder {
        Decoder { inner: vp9::Decoder::new(threads), input: Vec::new(), held: None, picture: [0; PICTURE_FIELDS] }
    }

    /// Room for a frame of `size` bytes in this module's memory, where the
    /// page writes it before [`Self::decode`]. Good until that call. The
    /// previous picture is released here.
    pub fn input(&mut self, size: usize) -> *mut u8 {
        self.held = None;
        self.input.clear();
        self.input.resize(size, 0);
        self.input.as_mut_ptr()
    }

    /// Decode the frame written into [`Self::input`]: true when it is one
    /// that is shown, which [`Self::picture`] describes. Throws for a frame
    /// that does not decode, after which the stream waits for a keyframe.
    pub fn decode(&mut self) -> Result<bool, JsError> {
        let frame = std::mem::take(&mut self.input);
        let r = self.inner.decode(&frame);
        self.input = frame;
        self.held = r.map_err(|e| JsError::new(&e.to_string()))?;
        Ok(self.held.is_some())
    }

    /// The picture the last frame decoded to, in sixteen numbers, as
    /// hevc-wasm's module describes one: its width and height, `2` for 4:4:4,
    /// the colour range (`2` full, `1` limited), the matrix, primaries and
    /// transfer as ITU-T H.273 numbers them (`2` for unstated), each plane's
    /// start in this module's memory, each plane's stride, and whether it is a
    /// keyframe. The planes are good until the next [`Self::input`].
    pub fn picture(&mut self) -> *const i32 {
        let p = &mut self.picture;
        *p = [0; PICTURE_FIELDS];
        if let Some(d) = &self.held {
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
        }
        p.as_ptr()
    }
}

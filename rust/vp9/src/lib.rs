//! A decoder for the VP9 remotex and wlshare send a browser at 4:4:4: profile
//! 1 at 8 bits, every frame the size of its references, each block predicted
//! from one of them. It refuses every other shape of stream by name.
//!
//! Each frame decodes to three planes of bytes, with the size to show and the
//! colour the stream states. A frame's tiles decode in parallel on rayon's
//! pool when the decoder is made with threads.

mod bits;
mod convolve;
mod decoder;
mod error;
mod frame;
mod header;
mod intra;
mod itx;
mod lf;
mod loopfilter;
mod probs;
mod tables;
mod threads;
mod tile;

pub use decoder::{Decoded, Decoder};
pub use error::{Error, Result};
pub use frame::{Frame, Plane};
pub use header::Colour;

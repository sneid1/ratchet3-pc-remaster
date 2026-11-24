//! The game's movies decoded by the port's own code: the program stream (`pss`), the MPEG-2 video (`video`, with its bit reader,
//! code tables and inverse DCT) and the ADPCM sound track (`audio`). Written from ISO 13818-1 and 13818-2 and the game's PSS files
//! (their layout is `uyafmt::movies`); `cinema.rs` plays them in the window (and port/uyaref's `movie.rs` into its headless capture).

pub mod audio;
pub mod bits;
pub mod idct;
pub mod pss;
pub mod video;
pub mod vlc;

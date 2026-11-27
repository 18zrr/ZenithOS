use crate::{display, fs::volume};
use zenith_fs::FsError;

const NAME: &str = "desktop.prefs";

#[derive(Clone, Copy)]
pub struct Prefs {
    pub width: u16,
    pub height: u16,
    pub accent: u8,
    pub gradient: bool,
    pub sample_seconds: u8,
}

impl Prefs {
    pub const DEFAULT: Self = Self {
        width: 0,
        height: 0,
        accent: 0,
        gradient: false,
        sample_seconds: 1,
    };
    pub fn load() -> Self {
        volume(|fs| {
            let id = fs.resolve(NAME)?;
            let data = fs.read_range(id, 0, 12)?;
            let [b'Z', b'P', version, accent, wall, wl, wh, hl, hh, ref tail @ ..] = data[..]
            else {
                return Err(FsError::InvalidPath);
            };
            let sample_seconds = match (version, tail) {
                (1, []) => 1,
                (2, [seconds @ (1 | 2 | 5)]) => *seconds,
                _ => return Err(FsError::InvalidPath),
            };
            let (width, height) = (u16::from_le_bytes([wl, wh]), u16::from_le_bytes([hl, hh]));
            if accent as usize >= super::ACCENTS.len()
                || wall > 1
                || ((width, height) != (0, 0)
                    && display::validate(width as usize, height as usize).is_err())
            {
                return Err(FsError::InvalidPath);
            }
            Ok(Self {
                width,
                height,
                accent,
                gradient: wall == 1,
                sample_seconds,
            })
        })
        .unwrap_or(Self::DEFAULT)
    }
    pub fn save(self) -> Result<(), FsError> {
        let [wl, wh] = self.width.to_le_bytes();
        let [hl, hh] = self.height.to_le_bytes();
        volume(|fs| {
            let bytes = [
                b'Z',
                b'P',
                2,
                self.accent,
                self.gradient as u8,
                wl,
                wh,
                hl,
                hh,
                self.sample_seconds,
            ];
            match fs.resolve(NAME) {
                Ok(id) => fs.write(id, &bytes),
                Err(FsError::FileNotFound) => fs.create_with_data(NAME, &bytes).map(|_| ()),
                Err(e) => return Err(e),
            }
        })
    }
}

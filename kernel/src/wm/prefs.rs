use crate::{display, fs::volume};
use zenith_fs::FsError;

const NAME: &str = "desktop.prefs";

#[derive(Clone, Copy)]
pub struct Prefs {
    pub width: u16,
    pub height: u16,
    pub accent: u8,
    pub gradient: bool,
}

impl Prefs {
    pub const DEFAULT: Self = Self {
        width: 0,
        height: 0,
        accent: 0,
        gradient: false,
    };
    pub fn load() -> Self {
        volume(|fs| {
            let id = fs.resolve(NAME)?;
            let data = fs.read_range(id, 0, 12)?;
            let [b'Z', b'P', 1, accent, wall, wl, wh, hl, hh] = data[..] else {
                return Err(FsError::InvalidPath);
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
            })
        })
        .unwrap_or(Self::DEFAULT)
    }
    pub fn save(self) -> Result<(), FsError> {
        let [wl, wh] = self.width.to_le_bytes();
        let [hl, hh] = self.height.to_le_bytes();
        volume(|fs| {
            let id = match fs.resolve(NAME) {
                Ok(id) => id,
                Err(FsError::FileNotFound) => fs.create(NAME, false)?,
                Err(e) => return Err(e),
            };
            fs.write(
                id,
                &[
                    b'Z',
                    b'P',
                    1,
                    self.accent,
                    self.gradient as u8,
                    wl,
                    wh,
                    hl,
                    hh,
                ],
            )
        })
    }
}

//! Copy or move marked entries between folders. Each completed operation is
//! one FS transaction. Stop at the first error and retain the remaining items.
use super::*;
use alloc::collections::VecDeque;

pub(super) struct Transfer {
    cut: bool,
    pending: VecDeque<(String, u32)>,
}

impl Files {
    pub(super) fn toggle_mark(&mut self) {
        let Some((name, _)) = self.entries.get(self.selected) else {
            return;
        };
        if let Some(i) = self.marked.iter().position(|n| n == name) {
            self.marked.remove(i);
        } else {
            self.marked.push(name.clone());
        }
        self.status = format!("{} marked for Copy or Cut", self.marked.len());
    }

    pub(super) fn copy_selection(&mut self, cut: bool) {
        let names = if self.marked.is_empty() {
            let Some((name, _)) = self.entries.get(self.selected) else {
                return;
            };
            core::slice::from_ref(name)
        } else {
            &self.marked
        };
        let result = volume(|fs| {
            let mut pending = VecDeque::new();
            for name in names {
                let path = format!("{}/{}", self.tab.dir, name);
                let id = fs.resolve(&path)?;
                if !cut && fs.stat(id)?.1 {
                    return Err(FsError::InvalidPath);
                }
                pending.push_back((path, id));
            }
            Ok(pending)
        });
        match result {
            Ok(pending) => {
                self.status = format!(
                    "{} ready to {} | Ctrl+V in destination",
                    pending.len(),
                    if cut { "move" } else { "copy" }
                );
                self.transfer = Some(Transfer { cut, pending });
            }
            Err(e) => {
                self.transfer = None;
                self.status = format!("Selection unavailable: {e} (only files can be copied)");
            }
        }
    }

    pub(super) fn paste_selection(&mut self) {
        let Some(mut transfer) = self.transfer.take() else {
            self.status = "No entries on the file clipboard".into();
            return;
        };
        let total = transfer.pending.len();
        let mut remaining = transfer.pending.clone();
        let mut error = None;
        while let Some((path, expected)) = remaining.front() {
            let result = volume(|fs| {
                let id = fs.resolve(path)?;
                if id != *expected {
                    return Err(FsError::FileNotFound);
                }
                if transfer.cut {
                    return fs.move_into(path, &self.tab.dir);
                }
                if fs.stat(id)?.0 > 256 * 1024 {
                    return Err(FsError::DiskFull);
                }
                let data = fs.read(id)?;
                let name = path.rsplit('/').next().ok_or(FsError::InvalidPath)?;
                fs.create_with_data(&format!("{}/{}", self.tab.dir, name), &data)?;
                Ok(())
            });
            if let Err(e) = result {
                error = Some(format!(
                    "{} of {total} done; {path}: {e}",
                    total - remaining.len()
                ));
                break;
            }
            remaining.pop_front();
        }
        self.tab.query.clear();
        self.refresh();
        self.status = error.unwrap_or_else(|| format!("{total} entries transferred"));
        crate::notifications::post("Explorer", &self.status);
        if !remaining.is_empty() {
            transfer.pending = remaining;
            self.transfer = Some(transfer);
        } else if !transfer.cut {
            self.transfer = Some(transfer);
        }
    }
}

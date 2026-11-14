use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{FileExt, OpenOptionsExt};

pub(crate) fn anonymous_file() -> io::Result<File> {
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let name = random.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let path = std::env::temp_dir().join(format!("lz-{}-{name}", std::process::id()));
    let file = OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path)?;
    std::fs::remove_file(path)?;
    Ok(file)
}

pub(crate) struct Offsets {
    memory: Vec<u64>,
    file: Option<File>,
    len: usize,
    last: u64,
}

impl Offsets {
    pub fn new() -> Self { Self { memory: vec![0], file: None, len: 1, last: 0 } }

    pub fn len(&self) -> usize { self.len }

    pub fn last(&self) -> u64 { self.last }

    pub fn push(&mut self, offset: u64) -> io::Result<()> {
        if self.file.is_none() && self.memory.len() == 8192 {
            let file = anonymous_file()?;
            for (index, value) in self.memory.iter().enumerate() {
                file.write_all_at(&value.to_le_bytes(), index as u64 * 8)?;
            }
            self.file = Some(file);
            self.memory = Vec::new();
        }
        if let Some(file) = &self.file { file.write_all_at(&offset.to_le_bytes(), self.len as u64 * 8)?; }
        else { self.memory.push(offset); }
        self.len += 1;
        self.last = offset;
        Ok(())
    }

    pub fn get(&self, index: usize) -> io::Result<u64> {
        if index >= self.len { return Err(io::Error::new(io::ErrorKind::InvalidInput, "line index outside scanned source")); }
        if let Some(file) = &self.file {
            let mut bytes = [0; 8];
            file.read_exact_at(&mut bytes, index as u64 * 8)?;
            Ok(u64::from_le_bytes(bytes))
        } else { Ok(self.memory[index]) }
    }

    /// Count the entries not exceeding `offset` (binary search).
    pub fn partition_point(&self, offset: u64) -> io::Result<usize> {
        let mut left = 0usize;
        let mut right = self.len;
        while left < right {
            let middle = left + (right - left) / 2;
            if self.get(middle)? <= offset { left = middle + 1; }
            else { right = middle; }
        }
        Ok(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_spills_and_preserves_random_access() {
        let mut offsets = Offsets::new();
        for value in 1..20_000 { offsets.push(value * 37).unwrap(); }
        assert_eq!(offsets.len(), 20_000);
        assert!(offsets.memory.is_empty());
        assert!(offsets.file.is_some());
        for index in [0, 1, 8191, 8192, 19_999] { assert_eq!(offsets.get(index).unwrap(), index as u64 * 37); }
        assert!(offsets.get(20_000).is_err());
    }
}

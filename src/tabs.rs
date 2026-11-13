/// Tab stops and their repeated spacing after the last explicit stop.
#[derive(Clone)]
pub struct TabStops { stops: Vec<usize>, interval: usize }

impl Default for TabStops {
    fn default() -> Self { Self { stops: vec![8], interval: 8 } }
}

impl TabStops {
    /// Parse the reference tab-stop syntax, retaining increasing positive stops.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let mut stops = Vec::new();
        let mut cursor = 0;
        while stops.len() < 127 {
            while bytes.get(cursor) == Some(&b' ') { cursor += 1; }
            let mut number = Some(0i32);
            while let Some(byte) = bytes.get(cursor).filter(|byte| byte.is_ascii_digit()) {
                number = number.and_then(|value| value.checked_mul(10)?.checked_add(i32::from(*byte - b'0')));
                cursor += 1;
            }
            if let Some(value) = number && value > 0 && stops.last().is_none_or(|last| value as usize > *last) {
                stops.push(value as usize);
            }
            while bytes.get(cursor) == Some(&b' ') { cursor += 1; }
            if bytes.get(cursor) != Some(&b',') { break; }
            cursor += 1;
        }
        let last = *stops.last()?;
        let previous = stops.iter().rev().nth(1).copied().unwrap_or(0);
        Some(Self { stops, interval: last - previous })
    }

    pub(crate) fn spaces_after(&self, column: usize) -> usize {
        if let Some(next) = self.stops.iter().copied().find(|stop| *stop > column) { return next - column; }
        let last = *self.stops.last().unwrap();
        self.interval - (column - last) % self.interval
    }
}

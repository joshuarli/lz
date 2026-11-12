use serde::{Deserialize, Serialize};
use std::io::{self, Write};

const BLOCK_BYTES: usize = 64 * 1024;
const INLINE_BYTES: usize = 1024 * 1024;
const GENERATED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub(super) enum Fixture { Bytes(Vec<u8>), Repeated(Repeat) }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Repeat {
    pub repeat: Vec<u8>,
    pub count: u64,
    pub suffix: Vec<u8>,
}

impl From<Vec<u8>> for Fixture { fn from(bytes: Vec<u8>) -> Self { Self::Bytes(bytes) } }

impl Fixture {
    pub fn validate(&self) -> io::Result<()> {
        let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
        match self {
            Self::Bytes(bytes) => if bytes.len() > INLINE_BYTES { return Err(invalid("large inline fixture requires a streaming recipe")); },
            Self::Repeated(recipe) => {
                if recipe.repeat.len() > BLOCK_BYTES || recipe.suffix.len() > INLINE_BYTES || (recipe.count != 0 && recipe.repeat.is_empty()) {
                    return Err(invalid("invalid or oversized repeat unit/suffix"));
                }
                let length = (recipe.repeat.len() as u64).checked_mul(recipe.count).and_then(|length| length.checked_add(recipe.suffix.len() as u64));
                if length.is_none_or(|length| length > GENERATED_BYTES) { return Err(invalid("generated fixture exceeds its 2 GiB budget")); }
            }
        }
        Ok(())
    }

    pub fn write_to(&self, output: &mut impl Write) -> io::Result<()> {
        self.validate()?;
        match self {
            Self::Bytes(bytes) => output.write_all(bytes),
            Self::Repeated(recipe) => {
                if recipe.count > 0 {
                    let repetitions = BLOCK_BYTES / recipe.repeat.len();
                    let block = recipe.repeat.repeat(repetitions);
                    for _ in 0..recipe.count / repetitions as u64 { output.write_all(&block)?; }
                    let remainder = (recipe.count % repetitions as u64) as usize * recipe.repeat.len();
                    output.write_all(&block[..remainder])?;
                }
                output.write_all(&recipe.suffix)
            }
        }
    }
}

pub(super) fn selftest() -> super::Result<()> {
    let recipe = Fixture::Repeated(Repeat { repeat: vec![0, 255, 13, 10, 42], count: 20_003, suffix: b"tail".to_vec() });
    let mut output = Vec::new();
    recipe.write_to(&mut output)?;
    let mut expected = [0, 255, 13, 10, 42].repeat(20_003);
    expected.extend_from_slice(b"tail");
    if output != expected { return Err("streamed fixture differs across 64 KiB boundaries".into()); }
    let empty = Fixture::Repeated(Repeat { repeat: vec![], count: 1, suffix: vec![] });
    let oversized = Fixture::Repeated(Repeat { repeat: vec![1], count: u64::MAX, suffix: vec![] });
    if empty.validate().is_ok() || oversized.validate().is_ok() { return Err("invalid repeat resource bounds accepted".into()); }
    if serde_json::from_value::<Fixture>(serde_json::json!({"repeat":[1],"count":1,"suffix":[],"unknown":true})).is_ok() {
        return Err("unknown fixture recipe field accepted".into());
    }
    println!("streamed fixture bytes match across block boundaries; empty/oversized recipes rejected");
    Ok(())
}

use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct ClassKey {
    pub class_id: u32,
    pub wrapped_key: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Keybag {
    pub dpsl: Vec<u8>,
    pub dpic: u32,
    pub salt: Vec<u8>,
    pub iter: u32,
    pub class_keys: Vec<ClassKey>,
}

/// Parse Apple TLV keybag structure from Manifest.plist.
pub fn parse_keybag(buf: &[u8]) -> Result<Keybag> {
    let mut i = 0;
    let mut dpsl = None;
    let mut dpic = None;
    let mut salt = None;
    let mut iter = None;

    let mut current_class = None;
    let mut current_wrapped = None;
    let mut class_keys = Vec::new();

    while i + 8 <= buf.len() {
        let tag = &buf[i..i + 4];
        i += 4;
        let len = u32::from_be_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if i + len > buf.len() {
            return Err(anyhow!("Keybag TLV length out of bounds"));
        }
        let val = &buf[i..i + len];
        i += len;

        match tag {
            b"DPSL" => dpsl = Some(val.to_vec()),
            b"DPIC" => {
                dpic = Some(parse_u32(tag, val)?);
            }
            b"SALT" => salt = Some(val.to_vec()),
            b"ITER" => {
                iter = Some(parse_u32(tag, val)?);
            }
            b"CLAS" => {
                if let (Some(c), Some(w)) = (current_class.take(), current_wrapped.take()) {
                    class_keys.push(ClassKey {
                        class_id: c,
                        wrapped_key: w,
                    });
                }
                current_class = Some(parse_u32(tag, val)?);
            }
            b"WPKY" => current_wrapped = Some(val.to_vec()),
            _ => {}
        }
    }

    if let (Some(c), Some(w)) = (current_class, current_wrapped) {
        class_keys.push(ClassKey {
            class_id: c,
            wrapped_key: w,
        });
    }

    Ok(Keybag {
        dpsl: dpsl.ok_or_else(|| anyhow!("Missing DPSL"))?,
        dpic: dpic.ok_or_else(|| anyhow!("Missing DPIC"))?,
        salt: salt.ok_or_else(|| anyhow!("Missing SALT"))?,
        iter: iter.ok_or_else(|| anyhow!("Missing ITER"))?,
        class_keys,
    })
}

fn parse_u32(tag: &[u8], val: &[u8]) -> Result<u32> {
    if val.len() != 4 {
        return Err(anyhow!(
            "Keybag tag {} expected 4 bytes, got {}",
            String::from_utf8_lossy(tag),
            val.len()
        ));
    }
    Ok(u32::from_be_bytes(val.try_into().unwrap()))
}

use aes_kw::KekAes256;
use anyhow::Result;
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;
use sha2::Sha256;
use std::collections::BTreeMap;
use zeroize::Zeroize;

use super::keybag::Keybag;

/// Owner-authorized key derivation (no brute force).
pub fn derive_class_keys(password: &str, kb: &Keybag) -> Result<BTreeMap<u32, Vec<u8>>> {
    let mut dpk = vec![0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &kb.dpsl, kb.dpic, &mut dpk);

    let mut kek_bytes = vec![0u8; 32];
    pbkdf2_hmac::<Sha1>(&dpk, &kb.salt, kb.iter, &mut kek_bytes);
    dpk.zeroize();

    let kek = KekAes256::try_from(kek_bytes.as_slice())?;
    kek_bytes.zeroize();

    let mut out = BTreeMap::new();
    for ck in &kb.class_keys {
        let key = kek.unwrap_vec(&ck.wrapped_key)?;
        out.insert(ck.class_id, key);
    }

    Ok(out)
}

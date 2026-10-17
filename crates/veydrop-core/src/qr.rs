//! QR code generation (module matrix; the UI renders it as SVG).

use crate::error::{Error, Result};
use crate::model::QrMatrix;

pub fn encode(text: &str) -> Result<QrMatrix> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| Error::Other(format!("qr: {e}")))?;
    let size = code.width();
    let modules: String = code
        .to_colors()
        .iter()
        .map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' })
        .collect();
    Ok(QrMatrix { size, modules })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_url() {
        let m = encode("http://192.168.1.20:47821/#t=abcdefghijklmnop").unwrap();
        assert_eq!(m.modules.len(), m.size * m.size);
        assert!(m.size >= 21);
        assert!(m.modules.starts_with("1111111"));
    }
}

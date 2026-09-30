use std::fmt;
use std::io;

#[derive(Debug)]
pub enum RecorderError {
    Io(io::Error),
    /// Falha de autenticação/decifragem (dado adulterado ou chave errada).
    Crypto,
    /// Estrutura inválida ou cadeia de integridade quebrada.
    Corrupt(&'static str),
    /// Limite de armazenamento atingido e nada pode ser removido.
    QuotaExceeded,
    KeyStore(String),
}

impl fmt::Display for RecorderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecorderError::Io(e) => write!(f, "io error: {e}"),
            RecorderError::Crypto => write!(f, "authentication/decryption failed"),
            RecorderError::Corrupt(m) => write!(f, "corrupt data: {m}"),
            RecorderError::QuotaExceeded => write!(f, "storage quota exceeded"),
            RecorderError::KeyStore(m) => write!(f, "key store error: {m}"),
        }
    }
}

impl std::error::Error for RecorderError {}

impl From<io::Error> for RecorderError {
    fn from(e: io::Error) -> Self {
        RecorderError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, RecorderError>;

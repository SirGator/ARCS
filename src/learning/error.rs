use crate::store::NetworkError;

#[derive(Debug)]
pub enum LearningError {
    Network(NetworkError),
}

impl From<NetworkError> for LearningError {
    fn from(value: NetworkError) -> Self {
        Self::Network(value)
    }
}

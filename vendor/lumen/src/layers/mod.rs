// 1. 声明子文件夹为模块
pub mod activation;
pub mod attention;
pub mod basic;
pub mod conv;
pub mod norm;
pub mod rnn;

// 2. 重新导出 (Re-export)
// 这样用户只需要 use rust_nn::layers::*; 就能拿到所有层
// 而不需要写 use rust_nn::layers::basic::linear::Linear;

// Basic
pub use basic::Dropout;
pub use basic::Embedding;
pub use basic::Flatten;
pub use basic::Linear;

// Conv
pub use conv::Conv2D;
pub use conv::MaxPool2D;

// RNN
pub use rnn::GRU;
pub use rnn::LSTM;
pub use rnn::RNN;

// Attention
pub use attention::KVCache;
pub use attention::RotaryEmbedding;
pub use attention::SelfAttention;

// Norm
pub use norm::RMSNorm;

pub use activation::Gelu;
pub use activation::SiLU;

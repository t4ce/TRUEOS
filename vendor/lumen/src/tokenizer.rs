use std::error::Error;
use std::path::Path;
use tokenizers::Tokenizer as HFTokenizer;

pub struct LlamaTokenizer {
    inner: HFTokenizer,
}

impl LlamaTokenizer {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let tokenizer =
            HFTokenizer::from_file(path).map_err(|e| Box::<dyn Error>::from(e.to_string()))?;
        Ok(Self { inner: tokenizer })
    }

    pub fn encode(
        &self,
        text: &str,
        with_special_tokens: bool,
    ) -> Result<Vec<usize>, Box<dyn Error>> {
        let encoding = self
            .inner
            .encode(text, with_special_tokens)
            .map_err(|e| Box::<dyn Error>::from(e.to_string()))?;
        Ok(encoding.get_ids().iter().map(|&id| id as usize).collect())
    }

    pub fn decode(&self, ids: &[usize], skip_special_tokens: bool) -> String {
        let ids_u32: Vec<u32> = ids.iter().map(|&id| id as u32).collect();
        self.inner
            .decode(&ids_u32, skip_special_tokens)
            .unwrap_or_else(|_| "".to_string())
    }

    pub fn vocab_size(&self) -> usize {
        self.inner.get_vocab_size(true)
    }

    pub fn token_to_id(&self, token: &str) -> Option<usize> {
        self.inner.token_to_id(token).map(|id| id as usize)
    }

    pub fn bos_id(&self) -> Option<usize> {
        // 常见 BOS
        for t in ["<s>", "<|begin_of_text|>"] {
            if let Some(id) = self.token_to_id(t) {
                return Some(id);
            }
        }
        None
    }

    pub fn eos_id(&self) -> Option<usize> {
        // 常见 EOS（Llama2/多数："</s>"；Llama3："<|end_of_text|>"；一些 chat："<|eot_id|>"）
        for t in ["</s>", "<|end_of_text|>", "<|eot_id|>"] {
            if let Some(id) = self.token_to_id(t) {
                return Some(id);
            }
        }
        None
    }

    pub fn eot_id(&self) -> Option<usize> {
        // 结束一轮对话的 token（Llama3 instruct 常见）
        self.token_to_id("<|eot_id|>")
    }

    pub fn pad_id(&self) -> Option<usize> {
        // 常见 PAD
        for t in ["<pad>", "<|pad|>"] {
            if let Some(id) = self.token_to_id(t) {
                return Some(id);
            }
        }
        None
    }
}

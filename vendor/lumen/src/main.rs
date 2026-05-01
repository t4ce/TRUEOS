use mimalloc::MiMalloc;

use lumen::autograd::{Device, Tensor, no_grad, set_strict_device_execution_scoped};
use lumen::init::{ParameterInitMode, with_parameter_init_mode};
use lumen::loader::{ModelLoader, WeightLoadOptions};
use lumen::models::{LlamaConfig, LlamaModel};
use lumen::module::Module;
use lumen::ops::fp_kernels::active_float_backend_name;
use lumen::ops::int8_kernels::active_int8_backend_name;
use lumen::precision::{
    DType, ParameterQuantization, PrecisionConfig, with_parameter_quantization,
    with_precision_config, with_runtime_component_dtypes,
};
use lumen::tokenizer::LlamaTokenizer;

use ndarray::{Array, Array1, Ix3, s};
use ndarray_rand::RandomExt;
use rand_distr::Uniform;

use std::env;
use std::io::{self, Write};
use std::path::Path;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Debug, Clone)]
struct Args {
    weights: String,
    tokenizer: String,
    system: String,
    temperature: f32,
    top_p: f32,
    repetition_penalty: f32,
    recent_window: usize,
    max_gen: usize,
    parameter_dtype: DType,
    runtime_dtype: DType,
    activation_dtype: DType,
    kv_cache_dtype: DType,
    allow_parameter_copies: bool,
    parameter_quantization: ParameterQuantization,
    stream_weights: bool,
    max_seq_len: usize,
    load_only: bool,
    device: Device,
}

fn usage(program: &str) {
    eprintln!(
        "Usage:\n  {program} --weights PATH --tokenizer PATH [options]\n\nOptions:\n  --system TEXT              System prompt\n  --temperature FLOAT        Sampling temperature (default: 0.8)\n  --top-p FLOAT              Top-p nucleus sampling (default: 0.9)\n  --repetition-penalty FLOAT Repetition penalty (default: 1.05)\n  --recent-window N          Recent token window for repetition penalty (default: 96)\n  --max-gen N                Max generated tokens per turn (default: 200)\n  --device DEVICE            cpu/cuda (default: cpu)\n  --parameter-dtype DTYPE    Default parameter dtype: f32/f16/bf16/i8 (default: f32)\n  --runtime-dtype DTYPE      Legacy shared default for activation/KV cache dtype: f32/f16/bf16 (default: f32)\n  --activation-dtype DTYPE   Activation/hidden dtype override: f32/f16/bf16/i8\n  --kv-cache-dtype DTYPE     KV cache dtype override: f32/f16/bf16\n  --quantize DTYPE           Quantize float weights on load: off/i8 (default: off)\n  --quant-scale FLOAT        Manual quantization scale override\n  --allow-parameter-copies   Allow cached parameter dtype copies\n  --stream-weights           Stream weights from disk instead of memory-mapping whole safetensors\n  --max-seq-len N            Override KV cache max sequence length (default: 2048)\n  --load-only                Load model and initialize KV cache, then exit\n\nCommands in chat:\n  /reset   Clear history and KV cache\n  /exit    Quit"
    );
}

fn model_config(max_seq_len: usize) -> LlamaConfig {
    LlamaConfig {
        vocab_size: 32000,
        hidden_size: 2048,
        intermediate_size: 5632,
        num_hidden_layers: 22,
        num_attention_heads: 32,
        num_key_value_heads: 4,
        rms_norm_eps: 1e-5,
        max_seq_len,
        rope_theta: 10000.0,
    }
}

fn parse_dtype_flag(flag: &str, value: &str, allow_integer: bool) -> Result<DType, String> {
    let dtype = match value.to_ascii_lowercase().as_str() {
        "f32" | "float32" => DType::F32,
        "f16" | "float16" | "half" => DType::F16,
        "bf16" | "bfloat16" => DType::BF16,
        "i8" | "int8" => DType::I8,
        other => {
            return Err(format!(
                "{flag} 不支持的 dtype: {other}，可选值为 f32/f16/bf16{}",
                if allow_integer { "/i8" } else { "" }
            ));
        }
    };

    if !allow_integer && dtype.is_integer() {
        return Err(format!("{flag} 当前只支持浮点 dtype，不能使用 {:?}", dtype));
    }

    Ok(dtype)
}

fn parse_args() -> Result<Args, String> {
    let argv: Vec<String> = env::args().collect();
    let program = argv.first().cloned().unwrap_or_else(|| "lumen".to_string());

    if argv.len() == 1 {
        usage(&program);
        return Err("缺少参数".to_string());
    }

    let mut weights: Option<String> = None;
    let mut tokenizer: Option<String> = None;
    let mut system = "You are a helpful AI assistant.".to_string();
    let mut temperature = 0.8f32;
    let mut top_p = 0.9f32;
    let mut repetition_penalty = 1.05f32;
    let mut recent_window = 96usize;
    let mut max_gen = 200usize;
    let mut parameter_dtype = DType::F32;
    let mut runtime_dtype = DType::F32;
    let mut activation_dtype: Option<DType> = None;
    let mut kv_cache_dtype: Option<DType> = None;
    let mut allow_parameter_copies = false;
    let mut quantize_dtype: Option<DType> = None;
    let mut quant_scale: Option<f32> = None;
    let mut stream_weights = false;
    let mut max_seq_len = 2048usize;
    let mut load_only = false;
    let mut device = Device::Cpu;

    let mut i = 1usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                usage(&program);
                std::process::exit(0);
            }
            "--weights" => {
                i += 1;
                weights = Some(argv.get(i).ok_or("--weights 缺少路径")?.clone());
            }
            "--tokenizer" => {
                i += 1;
                tokenizer = Some(argv.get(i).ok_or("--tokenizer 缺少路径")?.clone());
            }
            "--system" => {
                i += 1;
                system = argv.get(i).ok_or("--system 缺少文本")?.clone();
            }
            "--temperature" => {
                i += 1;
                temperature = argv
                    .get(i)
                    .ok_or("--temperature 缺少数字")?
                    .parse::<f32>()
                    .map_err(|_| "--temperature 需要 f32")?;
            }
            "--top-p" => {
                i += 1;
                top_p = argv
                    .get(i)
                    .ok_or("--top-p 缺少数字")?
                    .parse::<f32>()
                    .map_err(|_| "--top-p 需要 f32")?;
            }
            "--repetition-penalty" => {
                i += 1;
                repetition_penalty = argv
                    .get(i)
                    .ok_or("--repetition-penalty 缺少数字")?
                    .parse::<f32>()
                    .map_err(|_| "--repetition-penalty 需要 f32")?;
            }
            "--recent-window" => {
                i += 1;
                recent_window = argv
                    .get(i)
                    .ok_or("--recent-window 缺少数字")?
                    .parse::<usize>()
                    .map_err(|_| "--recent-window 需要 usize")?;
            }
            "--max-gen" => {
                i += 1;
                max_gen = argv
                    .get(i)
                    .ok_or("--max-gen 缺少数字")?
                    .parse::<usize>()
                    .map_err(|_| "--max-gen 需要 usize")?;
            }
            "--device" => {
                i += 1;
                device = match argv
                    .get(i)
                    .ok_or("--device 缺少设备")?
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "cpu" => Device::Cpu,
                    "cuda" | "gpu" => Device::Cuda,
                    other => return Err(format!("--device 不支持 {other}，可选 cpu/cuda")),
                };
            }
            "--parameter-dtype" => {
                i += 1;
                parameter_dtype = parse_dtype_flag(
                    "--parameter-dtype",
                    argv.get(i).ok_or("--parameter-dtype 缺少 dtype")?,
                    true,
                )?;
            }
            "--runtime-dtype" => {
                i += 1;
                runtime_dtype = parse_dtype_flag(
                    "--runtime-dtype",
                    argv.get(i).ok_or("--runtime-dtype 缺少 dtype")?,
                    false,
                )?;
            }
            "--activation-dtype" => {
                i += 1;
                activation_dtype = Some(parse_dtype_flag(
                    "--activation-dtype",
                    argv.get(i).ok_or("--activation-dtype 缺少 dtype")?,
                    true,
                )?);
            }
            "--kv-cache-dtype" => {
                i += 1;
                kv_cache_dtype = Some(parse_dtype_flag(
                    "--kv-cache-dtype",
                    argv.get(i).ok_or("--kv-cache-dtype 缺少 dtype")?,
                    false,
                )?);
            }
            "--quantize" => {
                i += 1;
                let raw = argv
                    .get(i)
                    .ok_or("--quantize 缺少 dtype，支持 off/i8")?
                    .to_ascii_lowercase();
                quantize_dtype = match raw.as_str() {
                    "off" | "none" | "disabled" => None,
                    _ => Some(parse_dtype_flag("--quantize", &raw, true)?),
                };
            }
            "--quant-scale" => {
                i += 1;
                quant_scale = Some(
                    argv.get(i)
                        .ok_or("--quant-scale 缺少数字")?
                        .parse::<f32>()
                        .map_err(|_| "--quant-scale 需要 f32")?,
                );
            }
            "--allow-parameter-copies" => {
                allow_parameter_copies = true;
            }
            "--stream-weights" => {
                stream_weights = true;
            }
            "--max-seq-len" => {
                i += 1;
                max_seq_len = argv
                    .get(i)
                    .ok_or("--max-seq-len 缺少数字")?
                    .parse::<usize>()
                    .map_err(|_| "--max-seq-len 需要 usize")?;
            }
            "--load-only" => {
                load_only = true;
            }
            other => return Err(format!("未知参数: {other}")),
        }
        i += 1;
    }

    if !(0.0..=1.0).contains(&top_p) {
        return Err("--top-p 必须在 [0, 1] 范围内".to_string());
    }
    if temperature < 0.0 {
        return Err("--temperature 不能小于 0".to_string());
    }
    if repetition_penalty < 1.0 {
        return Err("--repetition-penalty 不能小于 1.0".to_string());
    }
    if max_seq_len == 0 {
        return Err("--max-seq-len 必须 > 0".to_string());
    }
    if let Some(scale) = quant_scale {
        if !scale.is_finite() || scale <= 0.0 {
            return Err("--quant-scale 必须是有限且 > 0 的 f32".to_string());
        }
    }
    if let Some(dtype) = quantize_dtype {
        if !dtype.is_integer() {
            return Err(format!(
                "--quantize 当前只支持整数存储 dtype，收到 {:?}",
                dtype
            ));
        }
        if parameter_dtype != DType::F32 && parameter_dtype != dtype {
            return Err(format!(
                "--parameter-dtype={:?} 与 --quantize={:?} 冲突；量化开启时请设为默认 f32 或与量化 dtype 一致",
                parameter_dtype, dtype
            ));
        }
    } else if quant_scale.is_some() {
        return Err("--quant-scale 只有在 --quantize 开启时才可使用".to_string());
    }

    let parameter_quantization = match quantize_dtype {
        Some(dtype) => {
            let quant = ParameterQuantization::new(dtype);
            match quant_scale {
                Some(scale) => quant.with_scale(scale),
                None => quant,
            }
        }
        None => ParameterQuantization::Disabled,
    };
    let activation_dtype = activation_dtype.unwrap_or(runtime_dtype);
    let kv_cache_dtype = kv_cache_dtype.unwrap_or(runtime_dtype);

    Ok(Args {
        weights: weights.ok_or("必须提供 --weights")?,
        tokenizer: tokenizer.ok_or("必须提供 --tokenizer")?,
        system,
        temperature,
        top_p,
        repetition_penalty,
        recent_window,
        max_gen,
        parameter_dtype,
        runtime_dtype,
        activation_dtype,
        kv_cache_dtype,
        allow_parameter_copies,
        parameter_quantization,
        stream_weights,
        max_seq_len,
        load_only,
        device,
    })
}

fn build_first_turn_prompt(system: &str, user: &str) -> String {
    format!(
        "<|system|>\n{}\n</s>\n<|user|>\n{}\n</s>\n<|assistant|>\n",
        system, user
    )
}

fn build_next_turn_prompt(user: &str) -> String {
    format!("</s>\n<|user|>\n{}\n</s>\n<|assistant|>\n", user)
}

fn lcp_char_boundary(prev: &str, cur: &str) -> usize {
    let pb = prev.as_bytes();
    let cb = cur.as_bytes();
    let mut i = 0usize;
    let n = pb.len().min(cb.len());
    while i < n && pb[i] == cb[i] {
        i += 1;
    }
    while i > 0 && !cur.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn print_new_suffix(prev_printed: &mut String, cur_full: String) {
    if cur_full.contains('\u{FFFD}') {
        return;
    }
    let cut = lcp_char_boundary(prev_printed, &cur_full);
    if cut < cur_full.len() {
        print!("{}", &cur_full[cut..]);
        let _ = io::stdout().flush();
    }
    *prev_printed = cur_full;
}

#[inline]
fn rand01() -> f32 {
    Array1::<f32>::random(1, Uniform::new(0.0f32, 1.0f32))[0]
}

fn sample_top_p(
    logits: &[f32],
    temperature: f32,
    top_p: f32,
    repetition_penalty: f32,
    recent_tokens: &[usize],
) -> usize {
    let mut adjusted = logits
        .iter()
        .map(|&v| if v.is_finite() { v } else { f32::NEG_INFINITY })
        .collect::<Vec<_>>();

    if adjusted.is_empty() {
        return 0;
    }

    if repetition_penalty > 1.0 {
        for &t in recent_tokens {
            if t < adjusted.len() {
                adjusted[t] /= repetition_penalty;
            }
        }
    }

    if temperature <= 1e-5 {
        let mut best_i = 0usize;
        let mut best_v = f32::NEG_INFINITY;
        for (i, &v) in adjusted.iter().enumerate() {
            if v > best_v {
                best_v = v;
                best_i = i;
            }
        }
        return best_i;
    }

    for v in adjusted.iter_mut() {
        *v /= temperature;
    }

    let mut best_i = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    for (i, &v) in adjusted.iter().enumerate() {
        if v > best_v {
            best_v = v;
            best_i = i;
        }
    }
    if !best_v.is_finite() {
        return best_i;
    }

    let maxv = best_v;
    let mut probs: Vec<f32> = adjusted
        .iter()
        .map(|&x| if x.is_finite() { (x - maxv).exp() } else { 0.0 })
        .collect();
    let sum: f32 = probs.iter().sum();
    if !sum.is_finite() || sum <= 0.0 {
        return best_i;
    }
    let inv = 1.0 / sum;
    for p in probs.iter_mut() {
        *p *= inv;
    }

    let mut idxs: Vec<usize> = (0..probs.len()).collect();
    idxs.sort_by(|&i, &j| probs[j].total_cmp(&probs[i]));

    let mut cumulative = 0.0f32;
    let mut cut = 0usize;
    let target_p = top_p.clamp(0.0, 1.0).max(1e-6);
    for (rank, &i) in idxs.iter().enumerate() {
        cumulative += probs[i];
        cut = rank + 1;
        if cumulative >= target_p {
            break;
        }
    }
    idxs.truncate(cut.max(1));

    let r = rand01();
    let mut acc = 0.0f32;
    for &i in &idxs {
        acc += probs[i] / cumulative;
        if r <= acc {
            return i;
        }
    }
    *idxs.last().unwrap()
}

fn tensor_from_token_ids(ids: &[usize], device: Device) -> Tensor {
    let tensor = Tensor::from_array_no_grad(
        Array::from_shape_vec((1, ids.len()), ids.iter().map(|&x| x as f32).collect())
            .unwrap()
            .into_dyn(),
    );
    tensor.to_device(device)
}

fn last_step_logits_vec(logits: &Tensor) -> Vec<f32> {
    let logits_ref = logits.data_ref();
    let l3 = logits_ref
        .view()
        .into_dimensionality::<Ix3>()
        .expect("logits must be 3D [B,S,V]");
    let t = l3.shape()[1] - 1;
    l3.slice(s![0, t, ..]).iter().copied().collect()
}

fn env_flag_enabled(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args().map_err(|e| format!("参数错误: {e}"))?;
    let config = model_config(args.max_seq_len);

    if !Path::new(&args.tokenizer).exists() {
        return Err(format!("tokenizer 文件不存在: {}", args.tokenizer).into());
    }
    if !Path::new(&args.weights).exists() {
        return Err(format!("weights 文件不存在: {}", args.weights).into());
    }

    println!("Loading model...");
    println!(
        "  parameter_dtype={:?} activation_dtype={:?} kv_cache_dtype={:?} quantization={:?} stream_weights={} max_seq_len={} device={:?}",
        args.parameter_dtype,
        args.activation_dtype,
        args.kv_cache_dtype,
        args.parameter_quantization,
        args.stream_weights,
        args.max_seq_len,
        args.device
    );
    if env_flag_enabled("LUMEN_SHOW_BACKENDS") {
        println!(
            "  runtime_default={:?} fp_backend={} i8_backend={} allow_parameter_copies={}",
            args.runtime_dtype,
            active_float_backend_name(),
            active_int8_backend_name(),
            args.allow_parameter_copies,
        );
    }
    let tokenizer = LlamaTokenizer::from_file(&args.tokenizer)?;
    if tokenizer.vocab_size() != config.vocab_size {
        return Err(format!(
            "tokenizer vocab_size={} 与 model config vocab_size={} 不一致",
            tokenizer.vocab_size(),
            config.vocab_size
        )
        .into());
    }

    let precision_config = PrecisionConfig {
        parameter_dtype: args.parameter_dtype,
        runtime_dtype: args.runtime_dtype,
        allow_parameter_dtype_copies: args.allow_parameter_copies,
    };
    let load_options = WeightLoadOptions {
        float_source_quantization: args.parameter_quantization,
        stream_from_disk: args.stream_weights,
    };
    let model = with_precision_config(precision_config, || {
        with_runtime_component_dtypes(
            Some(args.activation_dtype),
            Some(args.kv_cache_dtype),
            || {
                with_parameter_quantization(args.parameter_quantization, || {
                    let model = with_parameter_init_mode(ParameterInitMode::Placeholder, || {
                        LlamaModel::new(config.clone())
                    });
                    println!("  weights={}", args.weights);
                    ModelLoader::load_llama_weights_with_options(
                        &args.weights,
                        &model.named_parameters(),
                        load_options,
                    )?;
                    Ok::<LlamaModel, Box<dyn std::error::Error>>(model)
                })
            },
        )
    })?;
    let _cuda_enabled = lumen::ops::cuda::set_enabled_scoped(args.device == Device::Cuda);
    let _strict_device_execution = set_strict_device_execution_scoped(args.device == Device::Cuda);
    if args.device == Device::Cuda {
        if !lumen::ops::cuda::is_available() {
            return Err(
                "CUDA 不可用；请确认使用 --features cuda 构建并安装 NVIDIA/CUDA 运行环境".into(),
            );
        }
        model.to_cuda();
    }

    println!("\nReady. Commands: /reset  /exit");

    let mut stop_ids: Vec<usize> = Vec::new();
    for t in ["</s>", "<|system|>", "<|user|>", "<|assistant|>"] {
        if let Some(id) = tokenizer.token_to_id(t) {
            stop_ids.push(id);
        }
    }
    if let Some(id) = tokenizer.eos_id() {
        stop_ids.push(id);
    }
    if let Some(id) = tokenizer.eot_id() {
        stop_ids.push(id);
    }
    stop_ids.sort_unstable();
    stop_ids.dedup();

    let mut all_tokens: Vec<usize> = Vec::new();
    let mut first_turn = true;

    let mut kv_caches = model.init_kv_caches(1);
    model.reset_kv_caches(&mut kv_caches);

    if args.load_only {
        println!("load-only complete.");
        return Ok(());
    }

    loop {
        print!("\n👤 User: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let user_msg = input.trim();

        if user_msg.is_empty() {
            continue;
        }
        if user_msg == "/exit" || user_msg == "exit" || user_msg == "quit" {
            break;
        }
        if user_msg == "/reset" || user_msg == "reset" {
            all_tokens.clear();
            model.reset_kv_caches(&mut kv_caches);
            first_turn = true;
            println!("reset done.");
            continue;
        }

        print!("🤖 Assistant: ");
        io::stdout().flush()?;

        no_grad(|| {
            let turn_prompt = if first_turn {
                build_first_turn_prompt(&args.system, user_msg)
            } else {
                build_next_turn_prompt(user_msg)
            };

            let mut new_tokens = match tokenizer.encode(&turn_prompt, false) {
                Ok(tokens) => tokens,
                Err(err) => {
                    eprintln!("tokenization failed: {err}");
                    println!();
                    return;
                }
            };

            if new_tokens.is_empty() {
                println!();
                first_turn = false;
                return;
            }

            let cur_len = kv_caches[0].borrow().len;
            if cur_len + new_tokens.len() + args.max_gen + 8 >= config.max_seq_len {
                all_tokens.clear();
                model.reset_kv_caches(&mut kv_caches);

                let prompt2 = build_first_turn_prompt(&args.system, user_msg);
                new_tokens = match tokenizer.encode(&prompt2, false) {
                    Ok(tokens) => tokens,
                    Err(err) => {
                        eprintln!("tokenization failed: {err}");
                        println!();
                        return;
                    }
                };
                if new_tokens.is_empty() {
                    println!();
                    first_turn = false;
                    return;
                }
            }

            all_tokens.extend_from_slice(&new_tokens);
            let assistant_start = all_tokens.len();

            let prefill_logits = model.forward_last_logits(
                tensor_from_token_ids(&new_tokens, args.device),
                &mut kv_caches,
                0,
            );
            let mut logits_vec = last_step_logits_vec(&prefill_logits);

            let mut prev_gen_text = String::new();
            for _ in 0..args.max_gen {
                let start = all_tokens.len().saturating_sub(args.recent_window);
                let recent = &all_tokens[start..];

                let next_token = sample_top_p(
                    &logits_vec,
                    args.temperature,
                    args.top_p,
                    args.repetition_penalty,
                    recent,
                );

                if stop_ids.contains(&next_token) {
                    break;
                }

                all_tokens.push(next_token);

                let gen_tokens = &all_tokens[assistant_start..];
                let cur_gen_text = tokenizer.decode(gen_tokens, true);
                if cur_gen_text.contains("<|user|>") || cur_gen_text.contains("<|assistant|>") {
                    break;
                }
                print_new_suffix(&mut prev_gen_text, cur_gen_text);

                if args.temperature <= 1e-5 && args.repetition_penalty <= 1.0 {
                    let next = model.forward_last_argmax(
                        tensor_from_token_ids(&[next_token], args.device),
                        &mut kv_caches,
                        0,
                    );
                    logits_vec.fill(f32::NEG_INFINITY);
                    if next < logits_vec.len() {
                        logits_vec[next] = 0.0;
                    }
                } else {
                    let logits2 = model.forward_last_logits(
                        tensor_from_token_ids(&[next_token], args.device),
                        &mut kv_caches,
                        0,
                    );
                    logits_vec = last_step_logits_vec(&logits2);
                }

                if kv_caches[0].borrow().len + 2 >= config.max_seq_len {
                    break;
                }
            }

            println!();
            first_turn = false;
        });
    }

    Ok(())
}

use mimalloc::MiMalloc;

use lumen::autograd::{Tensor, no_grad, set_strict_device_execution_scoped};
use lumen::layers::activation::Softmax;
use lumen::layers::{Conv2D, MaxPool2D, SelfAttention};
use lumen::loss::CrossEntropyLoss;
use lumen::models::{LlamaConfig, LlamaModel};
use lumen::module::Module;
use lumen::ops::arithmetic::sum;
use lumen::ops::fused::{
    fused_gate_up_silu_infer, fused_qkv_decode_infer_tensors, fused_qkv_prefill_infer_tensors,
};
use lumen::ops::matmul::matmul;
use lumen::optim::{Adam, Optimizer, SGD};
use lumen::precision::DType;

use ndarray::{Array, IxDyn};
use std::env;
use std::hint::black_box;
use std::time::{Duration, Instant};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Suite {
    All,
    Ops,
    Nn,
    Backward,
}

#[derive(Debug, Clone, Copy)]
enum SizeProfile {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone)]
struct Args {
    runs: usize,
    warmup: usize,
    suite: Suite,
    size: SizeProfile,
    dtype: DType,
    check: bool,
}

#[derive(Debug, Clone, Copy)]
struct ShapeConfig {
    matmul_m: usize,
    matmul_n: usize,
    matmul_k: usize,
    elem_len: usize,
    softmax_outer: usize,
    softmax_last: usize,
    conv_batch: usize,
    conv_in: usize,
    conv_out: usize,
    conv_hw: usize,
    attention_batch: usize,
    attention_seq: usize,
    attention_hidden: usize,
    attention_heads: usize,
    attention_kv_heads: usize,
}

#[derive(Debug, Clone)]
struct BenchResult {
    name: &'static str,
    cpu: Duration,
    cuda: Option<Duration>,
}

#[derive(Clone, Copy)]
struct BenchDef {
    name: &'static str,
    run: fn(&Args, ShapeConfig) -> BenchResult,
}

#[derive(Debug, Clone)]
struct CheckMetric {
    label: &'static str,
    abs: f32,
    rel: f32,
}

#[derive(Debug, Clone)]
struct CheckResult {
    name: &'static str,
    metrics: Vec<CheckMetric>,
}

#[derive(Clone, Copy)]
struct CheckDef {
    name: &'static str,
    run: fn(&Args, ShapeConfig) -> CheckResult,
}

struct BenchPlan<'a> {
    checks: &'a [CheckDef],
    benches: &'a [BenchDef],
}

impl BenchPlan<'_> {
    fn run(&self, args: &Args, cfg: ShapeConfig) {
        self.run_checks(args, cfg);
        self.run_benches(args, cfg);
    }

    fn run_checks(&self, args: &Args, cfg: ShapeConfig) {
        if !args.check {
            return;
        }
        if !lumen::ops::cuda::is_available() {
            println!("check: skipped because CUDA is unavailable.");
            return;
        }
        for check in self.checks {
            if should_run(args, check.name) {
                let result = (check.run)(args, cfg);
                print_check_result(&result);
            }
        }
    }

    fn run_benches(&self, args: &Args, cfg: ShapeConfig) {
        for bench in self.benches {
            if should_run(args, bench.name) {
                let result = (bench.run)(args, cfg);
                print_result(&result);
            }
        }
    }
}

fn usage(program: &str) {
    eprintln!(
        "Usage:\n  {program} [options]\n\nOptions:\n  --runs N       Timed runs per case (default: 10)\n  --warmup N     Warmup runs per case (default: 3)\n  --suite NAME   all/ops/nn/backward (default: all)\n  --size NAME    small/medium/large (default: medium)\n  --dtype DTYPE  f32/f16/bf16 (default: f32)\n  --check        Run CPU/CUDA correctness checks for supported cases\n\nExamples:\n  cargo run --release --features \"dev-tools cuda\" --bin cuda_cpu_bench -- --suite all --size medium --dtype bf16\n  cargo run --release --features \"dev-tools cuda\" --bin cuda_cpu_bench -- --suite nn --size small --dtype bf16 --check\n  cargo run --release --features \"dev-tools\" --bin cuda_cpu_bench -- --suite ops\n"
    );
}

fn parse_dtype(value: &str) -> Result<DType, String> {
    match value {
        "f32" => Ok(DType::F32),
        "f16" => Ok(DType::F16),
        "bf16" => Ok(DType::BF16),
        other => Err(format!(
            "未知 dtype: {other}; cuda_cpu_bench 目前支持 f32/f16/bf16"
        )),
    }
}

fn parse_args() -> Result<Args, String> {
    let argv = env::args().collect::<Vec<_>>();
    let program = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "cuda_cpu_bench".to_string());
    let mut runs = 10usize;
    let mut warmup = 3usize;
    let mut suite = Suite::All;
    let mut size = SizeProfile::Medium;
    let mut dtype = DType::F32;
    let mut check = false;

    let mut i = 1usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                usage(&program);
                std::process::exit(0);
            }
            "--runs" => {
                i += 1;
                runs = argv
                    .get(i)
                    .ok_or("--runs 缺少数字")?
                    .parse::<usize>()
                    .map_err(|_| "--runs 需要 usize")?;
                if runs == 0 {
                    return Err("--runs 必须 >= 1".to_string());
                }
            }
            "--warmup" => {
                i += 1;
                warmup = argv
                    .get(i)
                    .ok_or("--warmup 缺少数字")?
                    .parse::<usize>()
                    .map_err(|_| "--warmup 需要 usize")?;
            }
            "--suite" => {
                i += 1;
                suite = match argv.get(i).ok_or("--suite 缺少名称")?.as_str() {
                    "all" => Suite::All,
                    "ops" => Suite::Ops,
                    "nn" => Suite::Nn,
                    "backward" => Suite::Backward,
                    other => return Err(format!("未知 suite: {other}")),
                };
            }
            "--size" => {
                i += 1;
                size = match argv.get(i).ok_or("--size 缺少名称")?.as_str() {
                    "small" => SizeProfile::Small,
                    "medium" => SizeProfile::Medium,
                    "large" => SizeProfile::Large,
                    other => return Err(format!("未知 size: {other}")),
                };
            }
            "--dtype" => {
                i += 1;
                dtype = parse_dtype(argv.get(i).ok_or("--dtype 缺少名称")?.as_str())?;
            }
            "--check" => check = true,
            other => return Err(format!("未知参数: {other}")),
        }
        i += 1;
    }

    Ok(Args {
        runs,
        warmup,
        suite,
        size,
        dtype,
        check,
    })
}

fn shape_config(size: SizeProfile) -> ShapeConfig {
    match size {
        SizeProfile::Small => ShapeConfig {
            matmul_m: 256,
            matmul_n: 256,
            matmul_k: 256,
            elem_len: 1 << 18,
            softmax_outer: 512,
            softmax_last: 256,
            conv_batch: 2,
            conv_in: 8,
            conv_out: 16,
            conv_hw: 24,
            attention_batch: 1,
            attention_seq: 16,
            attention_hidden: 64,
            attention_heads: 4,
            attention_kv_heads: 2,
        },
        SizeProfile::Medium => ShapeConfig {
            matmul_m: 512,
            matmul_n: 512,
            matmul_k: 512,
            elem_len: 1 << 20,
            softmax_outer: 2048,
            softmax_last: 512,
            conv_batch: 4,
            conv_in: 16,
            conv_out: 32,
            conv_hw: 32,
            attention_batch: 2,
            attention_seq: 32,
            attention_hidden: 128,
            attention_heads: 8,
            attention_kv_heads: 4,
        },
        SizeProfile::Large => ShapeConfig {
            matmul_m: 1024,
            matmul_n: 1024,
            matmul_k: 1024,
            elem_len: 1 << 22,
            softmax_outer: 4096,
            softmax_last: 1024,
            conv_batch: 8,
            conv_in: 32,
            conv_out: 64,
            conv_hw: 48,
            attention_batch: 2,
            attention_seq: 64,
            attention_hidden: 256,
            attention_heads: 8,
            attention_kv_heads: 4,
        },
    }
}

fn sample_data(len: usize, scale: f32) -> Vec<f32> {
    (0..len)
        .map(|i| (((i * 17 + 11) % 97) as f32 - 48.0) * scale)
        .collect()
}

fn one_hot_data(rows: usize, cols: usize) -> Vec<f32> {
    let mut data = vec![0.0; rows * cols];
    for row in 0..rows {
        data[row * cols + (row * 13 + 7) % cols] = 1.0;
    }
    data
}

fn token_id_data(batch: usize, seq: usize, vocab_size: usize) -> Vec<f32> {
    (0..batch * seq)
        .map(|i| ((i * 7 + 3) % vocab_size) as f32)
        .collect()
}

fn llama_config(cfg: ShapeConfig) -> LlamaConfig {
    LlamaConfig {
        vocab_size: (cfg.attention_hidden * 2).max(64),
        hidden_size: cfg.attention_hidden,
        intermediate_size: cfg.attention_hidden * 4,
        num_hidden_layers: 1,
        num_attention_heads: cfg.attention_heads,
        num_key_value_heads: cfg.attention_kv_heads,
        rms_norm_eps: 1e-5,
        max_seq_len: cfg.attention_seq + 1,
        rope_theta: 10000.0,
    }
}

fn array_from_vec(shape: &[usize], data: Vec<f32>) -> ndarray::ArrayD<f32> {
    Array::from_shape_vec(IxDyn(shape), data)
        .expect("bench tensor shape mismatch")
        .into_dyn()
}

fn tensor_no_grad(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
    no_grad(|| Tensor::new_with_dtype(array_from_vec(shape, data), dtype))
}

fn tensor_grad(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
    Tensor::new_with_dtype(array_from_vec(shape, data), dtype)
}

fn tensor_const(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
    tensor_no_grad(shape, data, dtype)
}

fn token_tensor(shape: &[usize], data: Vec<f32>) -> Tensor {
    Tensor::from_array_no_grad(array_from_vec(shape, data))
}

fn median_duration(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn measure<F>(args: &Args, mut f: F) -> Duration
where
    F: FnMut(),
{
    for _ in 0..args.warmup {
        f();
    }
    let mut values = Vec::with_capacity(args.runs);
    for _ in 0..args.runs {
        let start = Instant::now();
        f();
        values.push(start.elapsed());
    }
    median_duration(values)
}

fn measure_cuda<F>(args: &Args, f: F) -> Option<Duration>
where
    F: FnMut(),
{
    if !lumen::ops::cuda::is_available() {
        return None;
    }
    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    let mut f = f;
    for _ in 0..args.warmup {
        f();
        lumen::ops::cuda::synchronize()
            .unwrap_or_else(|err| panic!("CUDA bench warmup sync failed: {err}"));
    }
    let mut values = Vec::with_capacity(args.runs);
    for _ in 0..args.runs {
        lumen::ops::cuda::synchronize()
            .unwrap_or_else(|err| panic!("CUDA bench pre-run sync failed: {err}"));
        let start = Instant::now();
        f();
        lumen::ops::cuda::synchronize()
            .unwrap_or_else(|err| panic!("CUDA bench timed sync failed: {err}"));
        values.push(start.elapsed());
    }
    let elapsed = median_duration(values);
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);
    Some(elapsed)
}

fn zero_all(tensors: &[&Tensor]) {
    for tensor in tensors {
        tensor.zero_grad();
    }
}

fn zero_params(module: &impl Module) {
    for param in module.parameters() {
        param.zero_grad();
    }
}

fn copy_parameters(src: &impl Module, dst: &impl Module) {
    let src_params = src.parameters();
    let dst_params = dst.parameters();
    assert_eq!(
        src_params.len(),
        dst_params.len(),
        "parameter count mismatch while preparing CUDA check"
    );
    for (src_param, dst_param) in src_params.iter().zip(dst_params.iter()) {
        let (shape, dtype, raw) = src_param.export_raw();
        dst_param
            .import_raw(shape, dtype, raw)
            .expect("parameter copy for CUDA check failed");
    }
}

fn dtype_check_tolerance(dtype: DType) -> (f32, f32) {
    match dtype {
        DType::F32 => (1e-2, 1e-2),
        DType::F16 | DType::BF16 => (3e-1, 8e-2),
        DType::I8 => (1.0, 2e-1),
    }
}

fn max_abs_rel_diff(lhs: &[f32], rhs: &[f32]) -> (f32, f32) {
    assert_eq!(lhs.len(), rhs.len(), "check vector length mismatch");
    let mut max_abs = 0.0f32;
    let mut max_rel = 0.0f32;
    for (&a, &b) in lhs.iter().zip(rhs.iter()) {
        let abs = (a - b).abs();
        let denom = a.abs().max(b.abs()).max(1e-6);
        max_abs = max_abs.max(abs);
        max_rel = max_rel.max(abs / denom);
    }
    (max_abs, max_rel)
}

fn assert_close_vec(
    label: &str,
    lhs: &[f32],
    rhs: &[f32],
    abs_tol: f32,
    rel_tol: f32,
) -> (f32, f32) {
    let (max_abs, max_rel) = max_abs_rel_diff(lhs, rhs);
    assert!(
        max_abs <= abs_tol || max_rel <= rel_tol,
        "{label} CPU/CUDA mismatch: max_abs={max_abs:.6e} max_rel={max_rel:.6e} abs_tol={abs_tol:.6e} rel_tol={rel_tol:.6e}"
    );
    (max_abs, max_rel)
}

fn check_metric(
    label: &'static str,
    lhs: &[f32],
    rhs: &[f32],
    abs_tol: f32,
    rel_tol: f32,
) -> CheckMetric {
    let (abs, rel) = assert_close_vec(label, lhs, rhs, abs_tol, rel_tol);
    CheckMetric { label, abs, rel }
}

fn collect_parameter_grads(module: &impl Module) -> Vec<Vec<f32>> {
    module
        .parameters()
        .into_iter()
        .map(|param| {
            param
                .grad()
                .expect("training check expected parameter grad")
                .iter()
                .copied()
                .collect()
        })
        .collect()
}

fn collect_parameter_data(module: &impl Module) -> Vec<Vec<f32>> {
    module
        .parameters()
        .into_iter()
        .map(|param| param.data_ref().iter().copied().collect())
        .collect()
}

fn tensor_data_vec(tensor: &Tensor) -> Vec<f32> {
    tensor.data_ref().iter().copied().collect()
}

fn tensor_grad_vec(tensor: &Tensor, label: &str) -> Vec<f32> {
    tensor
        .grad()
        .unwrap_or_else(|| panic!("{label} check expected tensor grad"))
        .iter()
        .copied()
        .collect()
}

fn llama_train_loss(
    model: &LlamaModel,
    input: Tensor,
    targets: &Tensor,
    vocab_size: usize,
) -> Tensor {
    let logits = model.forward_train(input);
    CrossEntropyLoss::apply(&logits.reshape(vec![-1, vocab_size as i32]), targets)
}

fn check_llama_train(args: &Args, cfg: ShapeConfig, step: bool, name: &'static str) -> CheckResult {
    assert!(
        lumen::ops::cuda::is_available(),
        "{name} check requires CUDA"
    );
    let llama_cfg = llama_config(cfg);
    let rows = cfg.attention_batch * cfg.attention_seq;
    let input = token_tensor(
        &[cfg.attention_batch, cfg.attention_seq],
        token_id_data(cfg.attention_batch, cfg.attention_seq, llama_cfg.vocab_size),
    );
    let targets = tensor_const(
        &[rows, llama_cfg.vocab_size],
        one_hot_data(rows, llama_cfg.vocab_size),
        args.dtype,
    );
    let cpu_model = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
    let cuda_model = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
    copy_parameters(&cpu_model, &cuda_model);

    zero_params(&cpu_model);
    let cpu_loss = llama_train_loss(&cpu_model, input.clone(), &targets, llama_cfg.vocab_size);
    let cpu_loss_value = cpu_loss.data_ref().first().copied().unwrap_or_default();
    cpu_loss.backward();
    let cpu_grads = collect_parameter_grads(&cpu_model);
    let mut cpu_step_model_data = Vec::new();
    if step {
        let mut cpu_opt = SGD::new(cpu_model.parameters(), 0.001);
        cpu_opt.step();
        cpu_step_model_data = collect_parameter_data(&cpu_model);
    }

    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    cuda_model.to_cuda();
    let input_cuda = input.to_cuda();
    let targets_cuda = targets.to_cuda();
    zero_params(&cuda_model);
    let cuda_loss = llama_train_loss(&cuda_model, input_cuda, &targets_cuda, llama_cfg.vocab_size);
    let cuda_loss_value = cuda_loss.data_ref().first().copied().unwrap_or_default();
    cuda_loss.backward();
    let cuda_grads = collect_parameter_grads(&cuda_model);
    let mut cuda_step_model_data = Vec::new();
    if step {
        let mut cuda_opt = SGD::new(cuda_model.parameters(), 0.001);
        cuda_opt.step();
        cuda_step_model_data = collect_parameter_data(&cuda_model);
    }
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);

    let (abs_tol, rel_tol) = dtype_check_tolerance(args.dtype);
    let loss_metric = check_metric(
        "loss",
        &[cpu_loss_value],
        &[cuda_loss_value],
        abs_tol,
        rel_tol,
    );
    let mut grad_max_abs = 0.0f32;
    let mut grad_max_rel = 0.0f32;
    assert_eq!(
        cpu_grads.len(),
        cuda_grads.len(),
        "training check grad parameter count mismatch"
    );
    for (idx, (cpu_grad, cuda_grad)) in cpu_grads.iter().zip(cuda_grads.iter()).enumerate() {
        let (abs, rel) = assert_close_vec(
            &format!("llama.train.grad[{idx}]"),
            cpu_grad,
            cuda_grad,
            abs_tol,
            rel_tol,
        );
        grad_max_abs = grad_max_abs.max(abs);
        grad_max_rel = grad_max_rel.max(rel);
    }

    let mut param_abs_rel = None;
    if step {
        let mut param_max_abs = 0.0f32;
        let mut param_max_rel = 0.0f32;
        assert_eq!(
            cpu_step_model_data.len(),
            cuda_step_model_data.len(),
            "training check parameter count mismatch after step"
        );
        for (idx, (cpu_param, cuda_param)) in cpu_step_model_data
            .iter()
            .zip(cuda_step_model_data.iter())
            .enumerate()
        {
            let (abs, rel) = assert_close_vec(
                &format!("llama.train.step.param[{idx}]"),
                cpu_param,
                cuda_param,
                abs_tol,
                rel_tol,
            );
            param_max_abs = param_max_abs.max(abs);
            param_max_rel = param_max_rel.max(rel);
        }
        param_abs_rel = Some((param_max_abs, param_max_rel));
    }

    let mut metrics = vec![
        loss_metric,
        CheckMetric {
            label: "grad",
            abs: grad_max_abs,
            rel: grad_max_rel,
        },
    ];
    if let Some((abs, rel)) = param_abs_rel {
        metrics.push(CheckMetric {
            label: "param",
            abs,
            rel,
        });
    }

    CheckResult { name, metrics }
}

fn check_llama_train_backward(args: &Args, cfg: ShapeConfig) -> CheckResult {
    check_llama_train(args, cfg, false, "llama.train.backward")
}

fn check_llama_train_step(args: &Args, cfg: ShapeConfig) -> CheckResult {
    check_llama_train(args, cfg, true, "llama.train.step")
}

fn check_matmul_backward(args: &Args, cfg: ShapeConfig) -> CheckResult {
    let (abs_tol, rel_tol) = dtype_check_tolerance(args.dtype);
    let a_data = sample_data(cfg.matmul_m * cfg.matmul_k, 0.01);
    let b_data = sample_data(cfg.matmul_k * cfg.matmul_n, -0.007);
    let coeff_data = sample_data(cfg.matmul_m * cfg.matmul_n, 0.003);

    let a_cpu = tensor_grad(&[cfg.matmul_m, cfg.matmul_k], a_data.clone(), args.dtype);
    let b_cpu = tensor_grad(&[cfg.matmul_k, cfg.matmul_n], b_data.clone(), args.dtype);
    let coeff_cpu = tensor_const(
        &[cfg.matmul_m, cfg.matmul_n],
        coeff_data.clone(),
        args.dtype,
    );
    let cpu_out = matmul(&a_cpu, &b_cpu);
    let cpu_loss = sum(&(&cpu_out * &coeff_cpu));
    let cpu_loss_value = cpu_loss.data_ref().first().copied().unwrap_or_default();
    cpu_loss.backward();
    let cpu_a_grad = tensor_grad_vec(&a_cpu, "matmul lhs");
    let cpu_b_grad = tensor_grad_vec(&b_cpu, "matmul rhs");

    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    let a_cuda = tensor_grad(&[cfg.matmul_m, cfg.matmul_k], a_data, args.dtype).to_cuda();
    let b_cuda = tensor_grad(&[cfg.matmul_k, cfg.matmul_n], b_data, args.dtype).to_cuda();
    let coeff_cuda = tensor_const(&[cfg.matmul_m, cfg.matmul_n], coeff_data, args.dtype).to_cuda();
    let cuda_out = matmul(&a_cuda, &b_cuda);
    let cuda_loss = sum(&(&cuda_out * &coeff_cuda));
    let cuda_loss_value = cuda_loss.data_ref().first().copied().unwrap_or_default();
    cuda_loss.backward();
    let cuda_a_grad = tensor_grad_vec(&a_cuda, "matmul CUDA lhs");
    let cuda_b_grad = tensor_grad_vec(&b_cuda, "matmul CUDA rhs");
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);

    let loss = check_metric(
        "loss",
        &[cpu_loss_value],
        &[cuda_loss_value],
        abs_tol,
        rel_tol,
    );
    let (a_abs, a_rel) = assert_close_vec(
        "matmul.backward.lhs_grad",
        &cpu_a_grad,
        &cuda_a_grad,
        abs_tol,
        rel_tol,
    );
    let (b_abs, b_rel) = assert_close_vec(
        "matmul.backward.rhs_grad",
        &cpu_b_grad,
        &cuda_b_grad,
        abs_tol,
        rel_tol,
    );
    CheckResult {
        name: "matmul.backward",
        metrics: vec![
            loss,
            CheckMetric {
                label: "grad",
                abs: a_abs.max(b_abs),
                rel: a_rel.max(b_rel),
            },
        ],
    }
}

fn check_cross_entropy_backward(args: &Args, cfg: ShapeConfig) -> CheckResult {
    let (abs_tol, rel_tol) = dtype_check_tolerance(args.dtype);
    let shape = [cfg.softmax_outer, cfg.softmax_last];
    let logits_data = sample_data(cfg.softmax_outer * cfg.softmax_last, 0.01);
    let target_data = one_hot_data(cfg.softmax_outer, cfg.softmax_last);

    let logits_cpu = tensor_grad(&shape, logits_data.clone(), args.dtype);
    let targets_cpu = tensor_const(&shape, target_data.clone(), args.dtype);
    let cpu_loss = CrossEntropyLoss::apply(&logits_cpu, &targets_cpu);
    let cpu_loss_value = cpu_loss.data_ref().first().copied().unwrap_or_default();
    cpu_loss.backward();
    let cpu_grad = tensor_grad_vec(&logits_cpu, "cross entropy logits");

    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    let logits_cuda = tensor_grad(&shape, logits_data, args.dtype).to_cuda();
    let targets_cuda = tensor_const(&shape, target_data, args.dtype).to_cuda();
    let cuda_loss = CrossEntropyLoss::apply(&logits_cuda, &targets_cuda);
    let cuda_loss_value = cuda_loss.data_ref().first().copied().unwrap_or_default();
    cuda_loss.backward();
    let cuda_grad = tensor_grad_vec(&logits_cuda, "cross entropy CUDA logits");
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);

    CheckResult {
        name: "cross_entropy.backward",
        metrics: vec![
            check_metric(
                "loss",
                &[cpu_loss_value],
                &[cuda_loss_value],
                abs_tol,
                rel_tol,
            ),
            check_metric("grad", &cpu_grad, &cuda_grad, abs_tol, rel_tol),
        ],
    }
}

fn check_elementwise_backward(args: &Args, cfg: ShapeConfig) -> CheckResult {
    let (abs_tol, rel_tol) = dtype_check_tolerance(args.dtype);
    let shape = [cfg.elem_len];
    let a_data = sample_data(cfg.elem_len, 0.01);
    let b_data = sample_data(cfg.elem_len, -0.02);

    let a_cpu = tensor_grad(&shape, a_data.clone(), args.dtype);
    let b_cpu = tensor_grad(&shape, b_data.clone(), args.dtype);
    let cpu_out = &(&a_cpu * &b_cpu) + &a_cpu;
    let cpu_loss = sum(&cpu_out);
    let cpu_loss_value = cpu_loss.data_ref().first().copied().unwrap_or_default();
    cpu_loss.backward();
    let cpu_a_grad = tensor_grad_vec(&a_cpu, "elementwise lhs");
    let cpu_b_grad = tensor_grad_vec(&b_cpu, "elementwise rhs");

    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    let a_cuda = tensor_grad(&shape, a_data, args.dtype).to_cuda();
    let b_cuda = tensor_grad(&shape, b_data, args.dtype).to_cuda();
    let cuda_out = &(&a_cuda * &b_cuda) + &a_cuda;
    let cuda_loss = sum(&cuda_out);
    let cuda_loss_value = cuda_loss.data_ref().first().copied().unwrap_or_default();
    cuda_loss.backward();
    let cuda_a_grad = tensor_grad_vec(&a_cuda, "elementwise CUDA lhs");
    let cuda_b_grad = tensor_grad_vec(&b_cuda, "elementwise CUDA rhs");
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);

    let (a_abs, a_rel) = assert_close_vec(
        "elementwise.backward.lhs_grad",
        &cpu_a_grad,
        &cuda_a_grad,
        abs_tol,
        rel_tol,
    );
    let (b_abs, b_rel) = assert_close_vec(
        "elementwise.backward.rhs_grad",
        &cpu_b_grad,
        &cuda_b_grad,
        abs_tol,
        rel_tol,
    );
    CheckResult {
        name: "elementwise.mul_add.backward",
        metrics: vec![
            check_metric(
                "loss",
                &[cpu_loss_value],
                &[cuda_loss_value],
                abs_tol,
                rel_tol,
            ),
            CheckMetric {
                label: "grad",
                abs: a_abs.max(b_abs),
                rel: a_rel.max(b_rel),
            },
        ],
    }
}

fn check_softmax_backward(args: &Args, cfg: ShapeConfig) -> CheckResult {
    let (abs_tol, rel_tol) = dtype_check_tolerance(args.dtype);
    let shape = [cfg.softmax_outer, cfg.softmax_last];
    let input_data = sample_data(cfg.softmax_outer * cfg.softmax_last, 0.01);
    let coeff_data = sample_data(cfg.softmax_outer * cfg.softmax_last, -0.004);
    let softmax = Softmax::new(1);

    let input_cpu = tensor_grad(&shape, input_data.clone(), args.dtype);
    let coeff_cpu = tensor_const(&shape, coeff_data.clone(), args.dtype);
    let cpu_out = softmax.forward(input_cpu.clone());
    let cpu_loss = sum(&(&cpu_out * &coeff_cpu));
    let cpu_loss_value = cpu_loss.data_ref().first().copied().unwrap_or_default();
    cpu_loss.backward();
    let cpu_grad = tensor_grad_vec(&input_cpu, "softmax input");

    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    let input_cuda = tensor_grad(&shape, input_data, args.dtype).to_cuda();
    let coeff_cuda = tensor_const(&shape, coeff_data, args.dtype).to_cuda();
    let cuda_out = softmax.forward(input_cuda.clone());
    let cuda_loss = sum(&(&cuda_out * &coeff_cuda));
    let cuda_loss_value = cuda_loss.data_ref().first().copied().unwrap_or_default();
    cuda_loss.backward();
    let cuda_grad = tensor_grad_vec(&input_cuda, "softmax CUDA input");
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);

    CheckResult {
        name: "softmax.backward",
        metrics: vec![
            check_metric(
                "loss",
                &[cpu_loss_value],
                &[cuda_loss_value],
                abs_tol,
                rel_tol,
            ),
            check_metric("grad", &cpu_grad, &cuda_grad, abs_tol, rel_tol),
        ],
    }
}

fn check_optimizer_step(
    args: &Args,
    cfg: ShapeConfig,
    adam: bool,
    state_dtype: DType,
    name: &'static str,
) -> CheckResult {
    let (abs_tol, rel_tol) = dtype_check_tolerance(args.dtype);
    let shape = [cfg.elem_len];
    let param_data = sample_data(cfg.elem_len, 0.01);
    let grad_data = sample_data(cfg.elem_len, -0.002);

    let param_cpu = tensor_grad(&shape, param_data.clone(), args.dtype);
    param_cpu.add_grad(grad_array(&shape, &grad_data));
    if adam {
        let mut opt = Adam::new_with_dtype(vec![param_cpu.clone()], 0.001, state_dtype);
        opt.step();
    } else {
        let mut opt = SGD::new(vec![param_cpu.clone()], 0.001);
        opt.step();
    }
    let cpu_param = tensor_data_vec(&param_cpu);

    let cuda_enabled_guard = lumen::ops::cuda::set_enabled_scoped(true);
    let strict_device_execution_guard = set_strict_device_execution_scoped(true);
    let param_cuda = tensor_grad(&shape, param_data, args.dtype).to_cuda();
    param_cuda.add_grad(grad_array(&shape, &grad_data));
    if adam {
        let mut opt = Adam::new_with_dtype(vec![param_cuda.clone()], 0.001, state_dtype);
        opt.step();
    } else {
        let mut opt = SGD::new(vec![param_cuda.clone()], 0.001);
        opt.step();
    }
    let cuda_param = tensor_data_vec(&param_cuda);
    drop(strict_device_execution_guard);
    drop(cuda_enabled_guard);

    CheckResult {
        name,
        metrics: vec![check_metric(
            "param",
            &cpu_param,
            &cuda_param,
            abs_tol,
            rel_tol,
        )],
    }
}

fn check_sgd_step(args: &Args, cfg: ShapeConfig) -> CheckResult {
    check_optimizer_step(args, cfg, false, args.dtype, "optimizer.sgd.step")
}

fn check_adam_step(args: &Args, cfg: ShapeConfig) -> CheckResult {
    check_optimizer_step(args, cfg, true, args.dtype, "optimizer.adam.step")
}

fn check_adam_f32_state_step(args: &Args, cfg: ShapeConfig) -> CheckResult {
    check_optimizer_step(args, cfg, true, DType::F32, "optimizer.adam_f32_state.step")
}

fn fused_qkv_prefill_cpu_reference(
    input: &Tensor,
    q_weight: &Tensor,
    k_weight: &Tensor,
    v_weight: &Tensor,
    batch: usize,
    seq: usize,
    hidden: usize,
    heads: usize,
    kv_heads: usize,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let head_dim = hidden / heads;
    let kv_hidden = kv_heads * head_dim;
    let x = input.data_ref().iter().copied().collect::<Vec<_>>();
    let q_w = q_weight.data_ref().iter().copied().collect::<Vec<_>>();
    let k_w = k_weight.data_ref().iter().copied().collect::<Vec<_>>();
    let v_w = v_weight.data_ref().iter().copied().collect::<Vec<_>>();
    let mut q_out = vec![0.0f32; batch * heads * seq * head_dim];
    let mut k_out = vec![0.0f32; batch * kv_heads * seq * head_dim];
    let mut v_out = vec![0.0f32; batch * kv_heads * seq * head_dim];

    for bb in 0..batch {
        for ss in 0..seq {
            let x_base = (bb * seq + ss) * hidden;
            for out_idx in 0..hidden {
                let mut sum = 0.0f32;
                for kk in 0..hidden {
                    sum += x[x_base + kk] * q_w[out_idx * hidden + kk];
                }
                let head = out_idx / head_dim;
                let dim = out_idx % head_dim;
                q_out[((bb * heads + head) * seq + ss) * head_dim + dim] = sum;
            }
            for out_idx in 0..kv_hidden {
                let mut k_sum = 0.0f32;
                let mut v_sum = 0.0f32;
                for kk in 0..hidden {
                    let x_val = x[x_base + kk];
                    k_sum += x_val * k_w[out_idx * hidden + kk];
                    v_sum += x_val * v_w[out_idx * hidden + kk];
                }
                let head = out_idx / head_dim;
                let dim = out_idx % head_dim;
                let offset = ((bb * kv_heads + head) * seq + ss) * head_dim + dim;
                k_out[offset] = k_sum;
                v_out[offset] = v_sum;
            }
        }
    }

    (q_out, k_out, v_out)
}

fn bench_matmul_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let a = tensor_no_grad(
        &[cfg.matmul_m, cfg.matmul_k],
        sample_data(cfg.matmul_m * cfg.matmul_k, 0.01),
        args.dtype,
    );
    let b = tensor_no_grad(
        &[cfg.matmul_k, cfg.matmul_n],
        sample_data(cfg.matmul_k * cfg.matmul_n, -0.007),
        args.dtype,
    );
    let cpu = measure(args, || {
        let out = no_grad(|| matmul(&a, &b));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let a_cuda = a.to_cuda();
        let b_cuda = b.to_cuda();
        measure_cuda(args, || {
            let out = no_grad(|| matmul(&a_cuda, &b_cuda));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "matmul.forward",
        cpu,
        cuda,
    }
}

fn bench_matmul_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let a = tensor_grad(
        &[cfg.matmul_m, cfg.matmul_k],
        sample_data(cfg.matmul_m * cfg.matmul_k, 0.01),
        args.dtype,
    );
    let b = tensor_grad(
        &[cfg.matmul_k, cfg.matmul_n],
        sample_data(cfg.matmul_k * cfg.matmul_n, -0.007),
        args.dtype,
    );
    let coeff = tensor_const(
        &[cfg.matmul_m, cfg.matmul_n],
        sample_data(cfg.matmul_m * cfg.matmul_n, 0.003),
        args.dtype,
    );
    let cpu = measure(args, || {
        zero_all(&[&a, &b]);
        let out = matmul(&a, &b);
        let loss = sum(&(&out * &coeff));
        loss.backward();
        black_box(a.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let a_cuda = a.to_cuda();
        let b_cuda = b.to_cuda();
        let coeff_cuda = coeff.to_cuda();
        measure_cuda(args, || {
            zero_all(&[&a_cuda, &b_cuda]);
            let out = matmul(&a_cuda, &b_cuda);
            let loss = sum(&(&out * &coeff_cuda));
            loss.backward();
            black_box(a_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "matmul.backward",
        cpu,
        cuda,
    }
}

fn bench_elementwise_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let a = tensor_no_grad(&[cfg.elem_len], sample_data(cfg.elem_len, 0.01), args.dtype);
    let b = tensor_no_grad(
        &[cfg.elem_len],
        sample_data(cfg.elem_len, -0.02),
        args.dtype,
    );
    let cpu = measure(args, || {
        let out = no_grad(|| &(&a * &b) + &a);
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let a_cuda = a.to_cuda();
        let b_cuda = b.to_cuda();
        measure_cuda(args, || {
            let out = no_grad(|| &(&a_cuda * &b_cuda) + &a_cuda);
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "elementwise.mul_add.forward",
        cpu,
        cuda,
    }
}

fn bench_elementwise_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let a = tensor_grad(&[cfg.elem_len], sample_data(cfg.elem_len, 0.01), args.dtype);
    let b = tensor_grad(
        &[cfg.elem_len],
        sample_data(cfg.elem_len, -0.02),
        args.dtype,
    );
    let cpu = measure(args, || {
        zero_all(&[&a, &b]);
        let out = &(&a * &b) + &a;
        let loss = sum(&out);
        loss.backward();
        black_box(a.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let a_cuda = a.to_cuda();
        let b_cuda = b.to_cuda();
        measure_cuda(args, || {
            zero_all(&[&a_cuda, &b_cuda]);
            let out = &(&a_cuda * &b_cuda) + &a_cuda;
            let loss = sum(&out);
            loss.backward();
            black_box(a_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "elementwise.mul_add.backward",
        cpu,
        cuda,
    }
}

fn bench_fused_gateup_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let hidden = cfg.attention_hidden;
    let inter = hidden * 4;
    let rows = cfg.attention_batch * cfg.attention_seq;
    let input = tensor_no_grad(
        &[rows, 1, hidden],
        sample_data(rows * hidden, 0.01),
        args.dtype,
    );
    let gate = tensor_no_grad(
        &[inter, hidden],
        sample_data(inter * hidden, 0.007),
        args.dtype,
    );
    let up = tensor_no_grad(
        &[inter, hidden],
        sample_data(inter * hidden, -0.005),
        args.dtype,
    );

    let cpu = measure(args, || {
        let out = no_grad(|| fused_gate_up_silu_infer(&input, &gate, &up));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        let gate_cuda = gate.to_cuda();
        let up_cuda = up.to_cuda();
        measure_cuda(args, || {
            let out = no_grad(|| fused_gate_up_silu_infer(&input_cuda, &gate_cuda, &up_cuda));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "fused_gateup.forward",
        cpu,
        cuda,
    }
}

fn bench_fused_qkv_decode(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let hidden = cfg.attention_hidden;
    let heads = cfg.attention_heads;
    let kv_heads = cfg.attention_kv_heads;
    let head_dim = hidden / heads;
    let kv_hidden = kv_heads * head_dim;
    let input = tensor_no_grad(
        &[cfg.attention_batch, 1, hidden],
        sample_data(cfg.attention_batch * hidden, 0.01),
        args.dtype,
    );
    let q = tensor_no_grad(
        &[hidden, hidden],
        sample_data(hidden * hidden, 0.007),
        args.dtype,
    );
    let k = tensor_no_grad(
        &[kv_hidden, hidden],
        sample_data(kv_hidden * hidden, -0.005),
        args.dtype,
    );
    let v = tensor_no_grad(
        &[kv_hidden, hidden],
        sample_data(kv_hidden * hidden, 0.003),
        args.dtype,
    );

    let cpu = measure(args, || {
        let (q_out, k_out, v_out) =
            no_grad(|| fused_qkv_decode_infer_tensors(&input, &q, &k, &v, heads, kv_heads));
        black_box((q_out.shape_vec(), k_out.shape_vec(), v_out.shape_vec()));
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        let q_cuda = q.to_cuda();
        let k_cuda = k.to_cuda();
        let v_cuda = v.to_cuda();
        measure_cuda(args, || {
            let (q_out, k_out, v_out) = no_grad(|| {
                fused_qkv_decode_infer_tensors(
                    &input_cuda,
                    &q_cuda,
                    &k_cuda,
                    &v_cuda,
                    heads,
                    kv_heads,
                )
            });
            black_box((q_out.shape_vec(), k_out.shape_vec(), v_out.shape_vec()));
        })
    } else {
        None
    };
    BenchResult {
        name: "fused_qkv.decode",
        cpu,
        cuda,
    }
}

fn bench_fused_qkv_prefill(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let batch = cfg.attention_batch;
    let seq = cfg.attention_seq;
    let hidden = cfg.attention_hidden;
    let heads = cfg.attention_heads;
    let kv_heads = cfg.attention_kv_heads;
    let head_dim = hidden / heads;
    let kv_hidden = kv_heads * head_dim;
    let input = tensor_no_grad(
        &[batch, seq, hidden],
        sample_data(batch * seq * hidden, 0.01),
        args.dtype,
    );
    let q = tensor_no_grad(
        &[hidden, hidden],
        sample_data(hidden * hidden, 0.007),
        args.dtype,
    );
    let k = tensor_no_grad(
        &[kv_hidden, hidden],
        sample_data(kv_hidden * hidden, -0.005),
        args.dtype,
    );
    let v = tensor_no_grad(
        &[kv_hidden, hidden],
        sample_data(kv_hidden * hidden, 0.003),
        args.dtype,
    );

    let cpu = measure(args, || {
        let out = fused_qkv_prefill_cpu_reference(
            &input, &q, &k, &v, batch, seq, hidden, heads, kv_heads,
        );
        black_box((out.0.len(), out.1.len(), out.2.len()));
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        let q_cuda = q.to_cuda();
        let k_cuda = k.to_cuda();
        let v_cuda = v.to_cuda();
        measure_cuda(args, || {
            let (q_out, k_out, v_out) = no_grad(|| {
                fused_qkv_prefill_infer_tensors(
                    &input_cuda,
                    &q_cuda,
                    &k_cuda,
                    &v_cuda,
                    heads,
                    kv_heads,
                )
                .expect("CUDA fused QKV prefill should run")
            });
            black_box((q_out.shape_vec(), k_out.shape_vec(), v_out.shape_vec()));
        })
    } else {
        None
    };
    BenchResult {
        name: "fused_qkv.prefill",
        cpu,
        cuda,
    }
}

fn bench_softmax_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let shape = [cfg.softmax_outer, cfg.softmax_last];
    let input = tensor_no_grad(
        &shape,
        sample_data(cfg.softmax_outer * cfg.softmax_last, 0.01),
        args.dtype,
    );
    let softmax = Softmax::new(1);

    let cpu = measure(args, || {
        let out = no_grad(|| softmax.forward(input.clone()));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        measure_cuda(args, || {
            let out = no_grad(|| softmax.forward(input_cuda.clone()));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "softmax.forward",
        cpu,
        cuda,
    }
}

fn bench_softmax_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let shape = [cfg.softmax_outer, cfg.softmax_last];
    let input = tensor_grad(
        &shape,
        sample_data(cfg.softmax_outer * cfg.softmax_last, 0.01),
        args.dtype,
    );
    let coeff = tensor_const(
        &shape,
        sample_data(cfg.softmax_outer * cfg.softmax_last, -0.004),
        args.dtype,
    );
    let softmax = Softmax::new(1);

    let cpu = measure(args, || {
        input.zero_grad();
        let out = softmax.forward(input.clone());
        let loss = sum(&(&out * &coeff));
        loss.backward();
        black_box(input.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        let coeff_cuda = coeff.to_cuda();
        measure_cuda(args, || {
            input_cuda.zero_grad();
            let out = softmax.forward(input_cuda.clone());
            let loss = sum(&(&out * &coeff_cuda));
            loss.backward();
            black_box(input_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "softmax.backward",
        cpu,
        cuda,
    }
}

fn bench_cross_entropy_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let shape = [cfg.softmax_outer, cfg.softmax_last];
    let logits = tensor_no_grad(
        &shape,
        sample_data(cfg.softmax_outer * cfg.softmax_last, 0.01),
        args.dtype,
    );
    let targets = tensor_const(
        &shape,
        one_hot_data(cfg.softmax_outer, cfg.softmax_last),
        args.dtype,
    );

    let cpu = measure(args, || {
        let loss = no_grad(|| CrossEntropyLoss::apply(&logits, &targets));
        black_box(loss.data_ref().first().copied().unwrap_or_default());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let logits_cuda = logits.to_cuda();
        let targets_cuda = targets.to_cuda();
        measure_cuda(args, || {
            let loss = no_grad(|| CrossEntropyLoss::apply(&logits_cuda, &targets_cuda));
            black_box(loss.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "cross_entropy.forward",
        cpu,
        cuda,
    }
}

fn bench_cross_entropy_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let shape = [cfg.softmax_outer, cfg.softmax_last];
    let logits = tensor_grad(
        &shape,
        sample_data(cfg.softmax_outer * cfg.softmax_last, 0.01),
        args.dtype,
    );
    let targets = tensor_const(
        &shape,
        one_hot_data(cfg.softmax_outer, cfg.softmax_last),
        args.dtype,
    );

    let cpu = measure(args, || {
        logits.zero_grad();
        let loss = CrossEntropyLoss::apply(&logits, &targets);
        loss.backward();
        black_box(logits.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let logits_cuda = logits.to_cuda();
        let targets_cuda = targets.to_cuda();
        measure_cuda(args, || {
            logits_cuda.zero_grad();
            let loss = CrossEntropyLoss::apply(&logits_cuda, &targets_cuda);
            loss.backward();
            black_box(logits_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "cross_entropy.backward",
        cpu,
        cuda,
    }
}

fn grad_array(shape: &[usize], data: &[f32]) -> ndarray::ArrayD<f32> {
    Array::from_shape_vec(IxDyn(shape), data.to_vec())
        .expect("bench grad shape mismatch")
        .into_dyn()
}

fn bench_sgd_step(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let shape = [cfg.elem_len];
    let param = tensor_grad(&shape, sample_data(cfg.elem_len, 0.01), args.dtype);
    let grad = sample_data(cfg.elem_len, -0.002);
    let mut opt = SGD::new(vec![param.clone()], 0.001);

    let cpu = measure(args, || {
        param.zero_grad();
        param.add_grad(grad_array(&shape, &grad));
        opt.step();
        black_box(param.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let param_cuda = param.to_cuda();
        let mut opt_cuda = SGD::new(vec![param_cuda.clone()], 0.001);
        measure_cuda(args, || {
            param_cuda.zero_grad();
            param_cuda.add_grad(grad_array(&shape, &grad));
            opt_cuda.step();
            black_box(param_cuda.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "optimizer.sgd.step",
        cpu,
        cuda,
    }
}

fn bench_adam_step(args: &Args, cfg: ShapeConfig) -> BenchResult {
    bench_adam_step_with_state_dtype(args, cfg, args.dtype, "optimizer.adam.step")
}

fn bench_adam_f32_state_step(args: &Args, cfg: ShapeConfig) -> BenchResult {
    bench_adam_step_with_state_dtype(args, cfg, DType::F32, "optimizer.adam_f32_state.step")
}

fn bench_adam_step_with_state_dtype(
    args: &Args,
    cfg: ShapeConfig,
    state_dtype: DType,
    name: &'static str,
) -> BenchResult {
    let shape = [cfg.elem_len];
    let param = tensor_grad(&shape, sample_data(cfg.elem_len, 0.01), args.dtype);
    let grad = sample_data(cfg.elem_len, -0.002);
    let mut opt = Adam::new_with_dtype(vec![param.clone()], 0.001, state_dtype);

    let cpu = measure(args, || {
        param.zero_grad();
        param.add_grad(grad_array(&shape, &grad));
        opt.step();
        black_box(param.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let param_cuda = param.to_cuda();
        let mut opt_cuda = Adam::new_with_dtype(vec![param_cuda.clone()], 0.001, state_dtype);
        measure_cuda(args, || {
            param_cuda.zero_grad();
            param_cuda.add_grad(grad_array(&shape, &grad));
            opt_cuda.step();
            black_box(param_cuda.shape_vec());
        })
    } else {
        None
    };
    BenchResult { name, cpu, cuda }
}

fn bench_conv2d_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let input = tensor_no_grad(
        &[cfg.conv_batch, cfg.conv_in, cfg.conv_hw, cfg.conv_hw],
        sample_data(
            cfg.conv_batch * cfg.conv_in * cfg.conv_hw * cfg.conv_hw,
            0.01,
        ),
        args.dtype,
    );
    let conv = Conv2D::new_with_dtype(cfg.conv_in, cfg.conv_out, 3, 1, 1, args.dtype);

    let cpu = measure(args, || {
        let out = no_grad(|| conv.forward(input.clone()));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let conv_cuda = Conv2D::new_with_dtype(cfg.conv_in, cfg.conv_out, 3, 1, 1, args.dtype);
        conv_cuda.to_cuda();
        let input_cuda = input.to_cuda();
        measure_cuda(args, || {
            let out = no_grad(|| conv_cuda.forward(input_cuda.clone()));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "conv2d.forward",
        cpu,
        cuda,
    }
}

fn bench_conv2d_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let input = tensor_grad(
        &[cfg.conv_batch, cfg.conv_in, cfg.conv_hw, cfg.conv_hw],
        sample_data(
            cfg.conv_batch * cfg.conv_in * cfg.conv_hw * cfg.conv_hw,
            0.01,
        ),
        args.dtype,
    );
    let coeff = tensor_const(
        &[cfg.conv_batch, cfg.conv_out, cfg.conv_hw, cfg.conv_hw],
        sample_data(
            cfg.conv_batch * cfg.conv_out * cfg.conv_hw * cfg.conv_hw,
            0.002,
        ),
        args.dtype,
    );
    let conv = Conv2D::new_with_dtype(cfg.conv_in, cfg.conv_out, 3, 1, 1, args.dtype);

    let cpu = measure(args, || {
        input.zero_grad();
        zero_params(&conv);
        let out = conv.forward(input.clone());
        let loss = sum(&(&out * &coeff));
        loss.backward();
        black_box(input.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let conv_cuda = Conv2D::new_with_dtype(cfg.conv_in, cfg.conv_out, 3, 1, 1, args.dtype);
        conv_cuda.to_cuda();
        let input_cuda = input.to_cuda();
        let coeff_cuda = coeff.to_cuda();
        measure_cuda(args, || {
            input_cuda.zero_grad();
            zero_params(&conv_cuda);
            let out = conv_cuda.forward(input_cuda.clone());
            let loss = sum(&(&out * &coeff_cuda));
            loss.backward();
            black_box(input_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "conv2d.backward",
        cpu,
        cuda,
    }
}

fn bench_max_pool_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let input = tensor_no_grad(
        &[cfg.conv_batch, cfg.conv_in, cfg.conv_hw, cfg.conv_hw],
        sample_data(
            cfg.conv_batch * cfg.conv_in * cfg.conv_hw * cfg.conv_hw,
            0.01,
        ),
        args.dtype,
    );
    let pool = MaxPool2D::new(2, 2);

    let cpu = measure(args, || {
        let out = no_grad(|| pool.forward(input.clone()));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        measure_cuda(args, || {
            let out = no_grad(|| pool.forward(input_cuda.clone()));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "max_pool2d.forward",
        cpu,
        cuda,
    }
}

fn bench_max_pool_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let out_hw = cfg.conv_hw / 2;
    let input = tensor_grad(
        &[cfg.conv_batch, cfg.conv_in, cfg.conv_hw, cfg.conv_hw],
        sample_data(
            cfg.conv_batch * cfg.conv_in * cfg.conv_hw * cfg.conv_hw,
            0.01,
        ),
        args.dtype,
    );
    let coeff = tensor_const(
        &[cfg.conv_batch, cfg.conv_in, out_hw, out_hw],
        sample_data(cfg.conv_batch * cfg.conv_in * out_hw * out_hw, 0.002),
        args.dtype,
    );
    let pool = MaxPool2D::new(2, 2);

    let cpu = measure(args, || {
        input.zero_grad();
        let out = pool.forward(input.clone());
        let loss = sum(&(&out * &coeff));
        loss.backward();
        black_box(input.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let input_cuda = input.to_cuda();
        let coeff_cuda = coeff.to_cuda();
        measure_cuda(args, || {
            input_cuda.zero_grad();
            let out = pool.forward(input_cuda.clone());
            let loss = sum(&(&out * &coeff_cuda));
            loss.backward();
            black_box(input_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "max_pool2d.backward",
        cpu,
        cuda,
    }
}

fn bench_attention_forward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let input = tensor_no_grad(
        &[cfg.attention_batch, cfg.attention_seq, cfg.attention_hidden],
        sample_data(
            cfg.attention_batch * cfg.attention_seq * cfg.attention_hidden,
            0.01,
        ),
        args.dtype,
    );
    let attn = SelfAttention::new_with_dtype(
        cfg.attention_hidden,
        cfg.attention_heads,
        cfg.attention_kv_heads,
        cfg.attention_seq,
        10000.0,
        true,
        args.dtype,
    );

    let cpu = measure(args, || {
        let (out, _) = no_grad(|| attn.forward(input.clone(), None));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let attn_cuda = SelfAttention::new_with_dtype(
            cfg.attention_hidden,
            cfg.attention_heads,
            cfg.attention_kv_heads,
            cfg.attention_seq,
            10000.0,
            true,
            args.dtype,
        );
        attn_cuda.to_cuda();
        let input_cuda = input.to_cuda();
        measure_cuda(args, || {
            let (out, _) = no_grad(|| attn_cuda.forward(input_cuda.clone(), None));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "self_attention.forward",
        cpu,
        cuda,
    }
}

fn bench_attention_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let input = tensor_grad(
        &[cfg.attention_batch, cfg.attention_seq, cfg.attention_hidden],
        sample_data(
            cfg.attention_batch * cfg.attention_seq * cfg.attention_hidden,
            0.01,
        ),
        args.dtype,
    );
    let coeff = tensor_const(
        &[cfg.attention_batch, cfg.attention_seq, cfg.attention_hidden],
        sample_data(
            cfg.attention_batch * cfg.attention_seq * cfg.attention_hidden,
            0.002,
        ),
        args.dtype,
    );
    let attn = SelfAttention::new_with_dtype(
        cfg.attention_hidden,
        cfg.attention_heads,
        cfg.attention_kv_heads,
        cfg.attention_seq,
        10000.0,
        true,
        args.dtype,
    );

    let cpu = measure(args, || {
        input.zero_grad();
        zero_params(&attn);
        let (out, _) = attn.forward(input.clone(), None);
        let loss = sum(&(&out * &coeff));
        loss.backward();
        black_box(input.grad().is_some());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let attn_cuda = SelfAttention::new_with_dtype(
            cfg.attention_hidden,
            cfg.attention_heads,
            cfg.attention_kv_heads,
            cfg.attention_seq,
            10000.0,
            true,
            args.dtype,
        );
        attn_cuda.to_cuda();
        let input_cuda = input.to_cuda();
        let coeff_cuda = coeff.to_cuda();
        measure_cuda(args, || {
            input_cuda.zero_grad();
            zero_params(&attn_cuda);
            let (out, _) = attn_cuda.forward(input_cuda.clone(), None);
            let loss = sum(&(&out * &coeff_cuda));
            loss.backward();
            black_box(input_cuda.requires_grad());
        })
    } else {
        None
    };
    BenchResult {
        name: "self_attention.backward",
        cpu,
        cuda,
    }
}

fn bench_llama_infer_last_logits(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let llama_cfg = llama_config(cfg);
    let input = token_tensor(
        &[cfg.attention_batch, cfg.attention_seq],
        token_id_data(cfg.attention_batch, cfg.attention_seq, llama_cfg.vocab_size),
    );
    let model = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
    let mut caches = model.init_kv_caches(cfg.attention_batch);

    let cpu = measure(args, || {
        model.reset_kv_caches(&mut caches);
        let out = no_grad(|| model.forward_last_logits(input.clone(), &mut caches, 0));
        black_box(out.shape_vec());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let model_cuda = LlamaModel::new_with_dtype(llama_cfg, args.dtype);
        model_cuda.to_cuda();
        let mut caches_cuda = model_cuda.init_kv_caches(cfg.attention_batch);
        let input_cuda = input.to_cuda();
        measure_cuda(args, || {
            model_cuda.reset_kv_caches(&mut caches_cuda);
            let out =
                no_grad(|| model_cuda.forward_last_logits(input_cuda.clone(), &mut caches_cuda, 0));
            black_box(out.shape_vec());
        })
    } else {
        None
    };
    BenchResult {
        name: "llama.infer_last_logits",
        cpu,
        cuda,
    }
}

fn bench_llama_prefill_decode(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let llama_cfg = llama_config(cfg);
    let prefill_input = token_tensor(
        &[cfg.attention_batch, cfg.attention_seq],
        token_id_data(cfg.attention_batch, cfg.attention_seq, llama_cfg.vocab_size),
    );
    let decode_input = token_tensor(
        &[cfg.attention_batch, 1],
        token_id_data(cfg.attention_batch, 1, llama_cfg.vocab_size),
    );
    let model = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
    let mut caches = model.init_kv_caches(cfg.attention_batch);

    let cpu = measure(args, || {
        model.reset_kv_caches(&mut caches);
        no_grad(|| {
            let prefill = model.forward_last_logits(prefill_input.clone(), &mut caches, 0);
            let decode =
                model.forward_last_logits(decode_input.clone(), &mut caches, cfg.attention_seq);
            black_box((prefill.shape_vec(), decode.shape_vec()));
        });
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let model_cuda = LlamaModel::new_with_dtype(llama_cfg, args.dtype);
        model_cuda.to_cuda();
        let mut caches_cuda = model_cuda.init_kv_caches(cfg.attention_batch);
        let prefill_cuda = prefill_input.to_cuda();
        let decode_cuda = decode_input.to_cuda();
        measure_cuda(args, || {
            model_cuda.reset_kv_caches(&mut caches_cuda);
            no_grad(|| {
                let prefill =
                    model_cuda.forward_last_logits(prefill_cuda.clone(), &mut caches_cuda, 0);
                let decode = model_cuda.forward_last_logits(
                    decode_cuda.clone(),
                    &mut caches_cuda,
                    cfg.attention_seq,
                );
                black_box((prefill.shape_vec(), decode.shape_vec()));
            });
        })
    } else {
        None
    };
    BenchResult {
        name: "llama.prefill_decode",
        cpu,
        cuda,
    }
}

fn bench_llama_train_backward(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let llama_cfg = llama_config(cfg);
    let rows = cfg.attention_batch * cfg.attention_seq;
    let input = token_tensor(
        &[cfg.attention_batch, cfg.attention_seq],
        token_id_data(cfg.attention_batch, cfg.attention_seq, llama_cfg.vocab_size),
    );
    let targets = tensor_const(
        &[rows, llama_cfg.vocab_size],
        one_hot_data(rows, llama_cfg.vocab_size),
        args.dtype,
    );
    let model = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);

    let cpu = measure(args, || {
        zero_params(&model);
        let logits = model.forward_train(input.clone());
        let loss = CrossEntropyLoss::apply(
            &logits.reshape(vec![-1, llama_cfg.vocab_size as i32]),
            &targets,
        );
        loss.backward();
        black_box(model.parameters().len());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let model_cuda = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
        model_cuda.to_cuda();
        let input_cuda = input.to_cuda();
        let targets_cuda = targets.to_cuda();
        measure_cuda(args, || {
            zero_params(&model_cuda);
            let logits = model_cuda.forward_train(input_cuda.clone());
            let loss = CrossEntropyLoss::apply(
                &logits.reshape(vec![-1, llama_cfg.vocab_size as i32]),
                &targets_cuda,
            );
            loss.backward();
            black_box(model_cuda.parameters().len());
        })
    } else {
        None
    };
    BenchResult {
        name: "llama.train.backward",
        cpu,
        cuda,
    }
}

fn bench_llama_train_step(args: &Args, cfg: ShapeConfig) -> BenchResult {
    let llama_cfg = llama_config(cfg);
    let rows = cfg.attention_batch * cfg.attention_seq;
    let input = token_tensor(
        &[cfg.attention_batch, cfg.attention_seq],
        token_id_data(cfg.attention_batch, cfg.attention_seq, llama_cfg.vocab_size),
    );
    let targets = tensor_const(
        &[rows, llama_cfg.vocab_size],
        one_hot_data(rows, llama_cfg.vocab_size),
        args.dtype,
    );
    let model = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
    let mut opt = SGD::new(model.parameters(), 0.001);

    let cpu = measure(args, || {
        zero_params(&model);
        let loss = llama_train_loss(&model, input.clone(), &targets, llama_cfg.vocab_size);
        loss.backward();
        opt.step();
        black_box(model.parameters().len());
    });
    let cuda = if lumen::ops::cuda::is_available() {
        let model_cuda = LlamaModel::new_with_dtype(llama_cfg.clone(), args.dtype);
        model_cuda.to_cuda();
        let input_cuda = input.to_cuda();
        let targets_cuda = targets.to_cuda();
        let mut opt_cuda = SGD::new(model_cuda.parameters(), 0.001);
        measure_cuda(args, || {
            zero_params(&model_cuda);
            let loss = llama_train_loss(
                &model_cuda,
                input_cuda.clone(),
                &targets_cuda,
                llama_cfg.vocab_size,
            );
            loss.backward();
            opt_cuda.step();
            black_box(model_cuda.parameters().len());
        })
    } else {
        None
    };
    BenchResult {
        name: "llama.train.step",
        cpu,
        cuda,
    }
}

fn should_run(args: &Args, name: &str) -> bool {
    match args.suite {
        Suite::All => true,
        Suite::Ops => {
            name.starts_with("matmul")
                || name.starts_with("elementwise")
                || name.starts_with("fused_")
                || name.starts_with("softmax")
                || name.starts_with("cross_entropy")
        }
        Suite::Nn => {
            name.starts_with("conv2d")
                || name.starts_with("max_pool")
                || name.starts_with("self_attention")
                || name.starts_with("llama")
        }
        Suite::Backward => {
            name.ends_with(".backward")
                || name.starts_with("optimizer")
                || name == "llama.train.step"
        }
    }
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn print_result(result: &BenchResult) {
    match result.cuda {
        Some(cuda) => {
            let speedup = result.cpu.as_secs_f64() / cuda.as_secs_f64();
            println!(
                "{:<32} cpu={:>9.3} ms  cuda={:>9.3} ms  speedup={:>7.2}x",
                result.name,
                ms(result.cpu),
                ms(cuda),
                speedup
            );
        }
        None => {
            println!(
                "{:<32} cpu={:>9.3} ms  cuda=   skipped  speedup=    n/a",
                result.name,
                ms(result.cpu)
            );
        }
    }
}

fn print_check_result(result: &CheckResult) {
    print!("check {:<26} ok", result.name);
    for metric in &result.metrics {
        print!(
            " {}_abs={:.3e} {}_rel={:.3e}",
            metric.label, metric.abs, metric.label, metric.rel
        );
    }
    println!();
}

fn main() {
    let args = parse_args().unwrap_or_else(|err| {
        eprintln!("error: {err}");
        std::process::exit(2);
    });
    let cfg = shape_config(args.size);
    println!(
        "cuda/cpu bench: suite={:?} size={:?} dtype={:?} runs={} warmup={} check={} cuda_available={}",
        args.suite,
        args.size,
        args.dtype,
        args.runs,
        args.warmup,
        args.check,
        lumen::ops::cuda::is_available()
    );
    if !lumen::ops::cuda::is_available() {
        println!(
            "note: CUDA unavailable or binary was not built with --features cuda; CUDA columns will be skipped."
        );
    }

    let benches = [
        BenchDef {
            name: "matmul.forward",
            run: bench_matmul_forward,
        },
        BenchDef {
            name: "matmul.backward",
            run: bench_matmul_backward,
        },
        BenchDef {
            name: "elementwise.mul_add.forward",
            run: bench_elementwise_forward,
        },
        BenchDef {
            name: "elementwise.mul_add.backward",
            run: bench_elementwise_backward,
        },
        BenchDef {
            name: "fused_gateup.forward",
            run: bench_fused_gateup_forward,
        },
        BenchDef {
            name: "fused_qkv.decode",
            run: bench_fused_qkv_decode,
        },
        BenchDef {
            name: "fused_qkv.prefill",
            run: bench_fused_qkv_prefill,
        },
        BenchDef {
            name: "softmax.forward",
            run: bench_softmax_forward,
        },
        BenchDef {
            name: "softmax.backward",
            run: bench_softmax_backward,
        },
        BenchDef {
            name: "cross_entropy.forward",
            run: bench_cross_entropy_forward,
        },
        BenchDef {
            name: "cross_entropy.backward",
            run: bench_cross_entropy_backward,
        },
        BenchDef {
            name: "optimizer.sgd.step",
            run: bench_sgd_step,
        },
        BenchDef {
            name: "optimizer.adam.step",
            run: bench_adam_step,
        },
        BenchDef {
            name: "optimizer.adam_f32_state.step",
            run: bench_adam_f32_state_step,
        },
        BenchDef {
            name: "conv2d.forward",
            run: bench_conv2d_forward,
        },
        BenchDef {
            name: "conv2d.backward",
            run: bench_conv2d_backward,
        },
        BenchDef {
            name: "max_pool2d.forward",
            run: bench_max_pool_forward,
        },
        BenchDef {
            name: "max_pool2d.backward",
            run: bench_max_pool_backward,
        },
        BenchDef {
            name: "self_attention.forward",
            run: bench_attention_forward,
        },
        BenchDef {
            name: "self_attention.backward",
            run: bench_attention_backward,
        },
        BenchDef {
            name: "llama.infer_last_logits",
            run: bench_llama_infer_last_logits,
        },
        BenchDef {
            name: "llama.prefill_decode",
            run: bench_llama_prefill_decode,
        },
        BenchDef {
            name: "llama.train.backward",
            run: bench_llama_train_backward,
        },
        BenchDef {
            name: "llama.train.step",
            run: bench_llama_train_step,
        },
    ];

    let checks = [
        CheckDef {
            name: "matmul.backward",
            run: check_matmul_backward,
        },
        CheckDef {
            name: "elementwise.mul_add.backward",
            run: check_elementwise_backward,
        },
        CheckDef {
            name: "softmax.backward",
            run: check_softmax_backward,
        },
        CheckDef {
            name: "cross_entropy.backward",
            run: check_cross_entropy_backward,
        },
        CheckDef {
            name: "optimizer.sgd.step",
            run: check_sgd_step,
        },
        CheckDef {
            name: "optimizer.adam.step",
            run: check_adam_step,
        },
        CheckDef {
            name: "optimizer.adam_f32_state.step",
            run: check_adam_f32_state_step,
        },
        CheckDef {
            name: "llama.train.backward",
            run: check_llama_train_backward,
        },
        CheckDef {
            name: "llama.train.step",
            run: check_llama_train_step,
        },
    ];

    BenchPlan {
        checks: &checks,
        benches: &benches,
    }
    .run(&args, cfg);
}

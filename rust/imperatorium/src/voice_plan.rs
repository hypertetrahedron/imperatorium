//! Which model the voice worker loads, and where. Compiled into rust/voice
//! (by `#[path]`) and tested here, where `cargo test` runs without whisper.

/// The loads to try, in order: (model, on the GPU).
///
/// As voice.py's Transcriber.load did: `device: cpu` runs the CPU model
/// (`cpu_model`, else `model`); otherwise the model on the GPU when there is
/// one, then - no GPU, or it could not have the model - the CPU model.
pub fn loads(device: &str, model: &str, cpu_model: &str, gpu_present: bool) -> Vec<(String, bool)> {
    let on_cpu = if cpu_model.is_empty() { model } else { cpu_model };
    let mut plan = Vec::new();
    if device != "cpu" && gpu_present {
        plan.push((model.to_string(), true));
    }
    plan.push((on_cpu.to_string(), false));
    plan
}

#[cfg(test)]
mod tests {
    use super::loads;

    fn plan(v: &[(&str, bool)]) -> Vec<(String, bool)> {
        v.iter().map(|(m, g)| (m.to_string(), *g)).collect()
    }

    #[test]
    fn a_cpu_config_starts_on_the_cpu_model() {
        assert_eq!(loads("cpu", "large-v3-turbo", "small", true), plan(&[("small", false)]));
        assert_eq!(loads("cpu", "large-v3-turbo", "small", false), plan(&[("small", false)]));
    }

    #[test]
    fn a_gpu_config_tries_the_gpu_then_the_cpu_model() {
        assert_eq!(
            loads("cuda", "large-v3-turbo", "small", true),
            plan(&[("large-v3-turbo", true), ("small", false)])
        );
    }

    #[test]
    fn without_a_gpu_it_goes_straight_to_the_cpu_model() {
        assert_eq!(loads("cuda", "large-v3-turbo", "small", false), plan(&[("small", false)]));
    }

    #[test]
    fn no_cpu_model_means_the_model_on_the_cpu() {
        assert_eq!(loads("cpu", "medium", "", true), plan(&[("medium", false)]));
    }
}

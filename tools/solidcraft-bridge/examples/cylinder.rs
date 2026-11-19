//! Generate an original analytic fixture; never uses a commercial CAD application.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("usage: cargo run --example cylinder -- OUTPUT.step")?;
    let result = std::thread::Builder::new()
        .stack_size(solidcraft_kernel::KERNEL_STACK)
        .spawn(move || -> Result<(), String> {
            solidcraft_kernel::start();
            let body = solidcraft_kernel::cylinder(solidcore_record::Vec3::ZERO, solidcore_record::Vec3::Z, 5.0, 20.0).map_err(|e| e.to_string())?;
            let text = solidcraft_kernel::step_export_bodies(
                &[solidcraft_kernel::ExportBody { name: "analytic cylinder".into(), body: &body, color: None }],
                &solidcraft_kernel::StepHeader {
                    description: "Original synthetic cylinder for CADCraft model-worker prototype".into(),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut file, text.as_bytes()).map_err(|e| e.to_string())
        })?
        .join();
    match result {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err("fixture generator kernel thread failed".into()),
    }
}

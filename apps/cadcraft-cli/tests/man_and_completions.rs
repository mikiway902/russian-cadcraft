//! Generated completion is executable for the installed product command, not upstream's name.

// The example itself, so the test runs its code; its `main` is unused here.
#[allow(dead_code)]
#[path = "../examples/man-and-completions.rs"]
mod example;

#[test]
fn command_is_consistent() {
    cadcraft_cli::command().debug_assert();
}

#[test]
#[cfg(unix)]
fn installed_cli_completes_a_subcommand_in_bash() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("man-and-completions-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    example::generate(&dir).unwrap();
    let result = std::process::Command::new("bash")
        .args([
            "-c",
            r#"
set -eu
source "$1"
read -r -a registration <<< "$(complete -p cadcraftgost-cli)"
function_name=
for ((i=0; i+1<${#registration[@]}; i++)); do
    if [[ ${registration[i]} == -F ]]; then
        function_name=${registration[i+1]}
        break
    fi
done
[[ -n $function_name ]]
COMP_WORDS=(cadcraftgost-cli con)
COMP_CWORD=1
COMP_LINE='cadcraftgost-cli con'
COMP_POINT=${#COMP_LINE}
"$function_name" cadcraftgost-cli con cadcraftgost-cli
printf '%s\n' "${COMPREPLY[@]}"
"#,
            "completion-smoke",
        ])
        .arg(dir.join("cadcraftgost-cli.bash"))
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert!(String::from_utf8_lossy(&result.stdout).lines().any(|candidate| candidate == "convert"));
    std::fs::remove_dir_all(&dir).unwrap();
}

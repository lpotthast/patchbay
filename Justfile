set dotenv-load := true

database := env_var_or_default("PATCHBAY_DATABASE", env_var_or_default("HOME", ".") / ".patchbay/patchbay.sqlite3")
bind := env_var_or_default("PATCHBAY_BIND", "127.0.0.1:4000")
project := env_var_or_default("PATCHBAY_PROJECT", "demo")
patchbay_cli_path := justfile_directory() / "dev-bin/patchbay"
server_manifest := "patchbay-server/Cargo.toml"
cli_manifest := "patchbay-cli/Cargo.toml"
api_client_manifest := "patchbay-api-client/Cargo.toml"
types_manifest := "patchbay-types/Cargo.toml"

default:
    @just --list

fmt:
    cargo fmt --manifest-path "{{server_manifest}}"
    cargo fmt --manifest-path "{{cli_manifest}}"
    cargo fmt --manifest-path "{{api_client_manifest}}"
    cargo fmt --manifest-path "{{types_manifest}}"

check:
    cargo check --manifest-path "{{server_manifest}}"
    cargo check --manifest-path "{{cli_manifest}}"
    cargo check --manifest-path "{{api_client_manifest}}"
    cargo check --manifest-path "{{types_manifest}}"

test:
    cargo test --manifest-path "{{server_manifest}}"
    cargo test --manifest-path "{{cli_manifest}}"
    cargo test --manifest-path "{{api_client_manifest}}"
    cargo test --manifest-path "{{types_manifest}}"

browser-test:
    cargo test --manifest-path "{{server_manifest}}" --test browser_test -- --nocapture

browser-test-visible:
    BROWSER_TEST_VISIBLE=1 cargo test --manifest-path "{{server_manifest}}" --test browser_test -- --nocapture

browser-test-pause:
    BROWSER_TEST_VISIBLE=1 BROWSER_TEST_PAUSE=1 cargo test --manifest-path "{{server_manifest}}" --test browser_test -- --nocapture

clippy:
    cargo clippy --manifest-path "{{server_manifest}}" --all-targets -- -D warnings
    cargo clippy --manifest-path "{{cli_manifest}}" --all-targets -- -D warnings
    cargo clippy --manifest-path "{{api_client_manifest}}" --all-targets -- -D warnings
    cargo clippy --manifest-path "{{types_manifest}}" --all-targets -- -D warnings

verify: fmt test clippy

verify-browser: verify browser-test

run *args:
    cargo run --manifest-path "{{server_manifest}}" -- {{args}}

cli *args:
    cargo run -q --manifest-path "{{cli_manifest}}" -- {{args}}

serve:
    PATCHBAY_CLI_PATH="{{patchbay_cli_path}}" cargo leptos --manifest-path "{{server_manifest}}" serve -- --database "{{database}}" --bind "{{bind}}"

serve-release:
    PATCHBAY_CLI_PATH="{{patchbay_cli_path}}" cargo leptos --manifest-path "{{server_manifest}}" serve --release -- --database "{{database}}" --bind "{{bind}}"

fresh-db:
    rm -f "{{database}}"

discover-tools:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" agent-tools discover

project-settings:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" project settings show --name "{{project}}"

automation-status:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" automation status --project "{{project}}"

automation-runs:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" automation runs --project "{{project}}"

automation-log run_id:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" automation log --project "{{project}}" {{run_id}}

recover-stale-claims:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" automation recover-stale-claims --project "{{project}}"

cleanup-worktrees:
    cargo run --manifest-path "{{server_manifest}}" -- --database "{{database}}" automation cleanup-worktrees --project "{{project}}"

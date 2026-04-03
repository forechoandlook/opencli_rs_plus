#!/bin/bash
# Re-run the tests to see the error output closely.
cargo test -p opencli-rs-daemon test_store_status_updates -- --nocapture
cargo test -p opencli-rs-daemon test_store_failed_and_retry -- --nocapture

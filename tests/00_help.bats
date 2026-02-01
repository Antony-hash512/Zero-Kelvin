#!/usr/bin/env bats

load "test_helper/bats-support/load"
load "test_helper/bats-assert/load"

setup() {
    # No special setup needed
    return 0
}

@test "Help: No arguments (Status 0, Show Help)" {
    run $ZKS_SQM_BIN
    assert_success
    assert_output --partial "Usage:"
    assert_output --partial "Commands:"
}

@test "Help: Invalid subcommand (Status 2, Show Help)" {
    run $ZKS_SQM_BIN invalid_cmd
    assert_failure 2
    assert_output --partial "error: unrecognized subcommand"
    assert_output --partial "Usage:"
    assert_output --partial "Commands:"
}

@test "Help: --help flag (Status 0)" {
    run $ZKS_SQM_BIN --help
    assert_success
    assert_output --partial "Usage:"
}

@test "Help: Create command descriptions" {
    run $ZKS_SQM_BIN create --help
    assert_success
    assert_output --partial "Path to the source directory"
    assert_output --partial "Disable variable progress bar"
}

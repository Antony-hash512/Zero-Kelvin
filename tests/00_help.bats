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
    assert_output --partial "Detailed Command Information:"
}

@test "Help: Invalid subcommand (Status 2, Show Help)" {
    run $ZKS_SQM_BIN invalid_cmd
    assert_failure 2
    assert_output --partial "error: unrecognized subcommand"
    assert_output --partial "Detailed Command Information:"
}

@test "Help: --help flag (Status 0)" {
    run $ZKS_SQM_BIN --help
    assert_success
    assert_output --partial "Detailed Command Information:"
}

@test "Help: Create command missing args (Show specific help)" {
    run $ZKS_SQM_BIN create
    assert_failure
    assert_output --partial "Usage: 0k-core create"
    assert_output --partial "Arguments:"
    assert_output --partial "<INPUT>"
}

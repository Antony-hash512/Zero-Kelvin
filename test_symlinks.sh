#!/usr/bin/env bash
set -e

# Path to binaries (Absolute)
ZKS="$(pwd)/target/debug/0k"
SQM="$(pwd)/target/debug/0k-core"

# Ensure binaries are in PATH for unshare
export PATH="$(pwd)/target/debug:$PATH"

TEST_DIR=$(mktemp -d)
echo "Testing in $TEST_DIR"
cd "$TEST_DIR"

mkdir -p source
echo "hello world" > source/real_file
ln -s real_file source/my_link

mkdir -p output

echo "--- Test 1: Default Freeze (Preserve Symlink) ---"
$ZKS freeze source/my_link output/test_preserve.sqfs
# Verify manifest in archive
mkdir -p mount_preserve
$SQM mount output/test_preserve.sqfs mount_preserve
grep "type: symlink" mount_preserve/list.yaml
ls -l mount_preserve/to_restore/1/my_link
$SQM umount mount_preserve

echo "--- Test 2: Check (Preserve Symlink) ---"
$ZKS check output/test_preserve.sqfs

echo "--- Test 3: Unfreeze (Preserve Symlink) ---"
mkdir -p restore_preserve
$ZKS unfreeze output/test_preserve.sqfs --skip-existing # It will restore to its original parent which is $TEST_DIR/source
# Wait, unfreeze restores to the path in manifest. 
# Original path was $TEST_DIR/source/my_link.
# Let's delete original to test unfreeze.
rm source/my_link
$ZKS unfreeze output/test_preserve.sqfs
ls -l source/my_link
[ -L source/my_link ] && echo "SUCCESS: Restored as symlink" || echo "FAIL: Not a symlink"

echo "--- Test 4: Dereference Freeze ---"
$ZKS freeze --dereference source/my_link output/test_deref.sqfs
mkdir -p mount_deref
$SQM mount output/test_deref.sqfs mount_deref
grep "type: file" mount_deref/list.yaml
ls -l mount_deref/to_restore/1/my_link
[ -f mount_deref/to_restore/1/my_link ] && [ ! -L mount_deref/to_restore/1/my_link ] && echo "SUCCESS: Dereferenced to file" || echo "FAIL: Not a file"
$SQM umount mount_deref

echo "--- Test 5: Check (Dereferenced) ---"
# Note: Check might fail if it expects a symlink but finds a file?
# Actually, if we froze it as a file, the manifest says 'file'. 
# When checking, it compares 'file' in archive vs 'symlink' in live?
# Mismatch (Type) would be expected if live is still symlink.
rm source/my_link
ln -s real_file source/my_link
$ZKS check output/test_deref.sqfs | grep "MISMATCH (Type)" && echo "SUCCESS: Mismatch detected as expected" || echo "FAIL: Mismatch not detected"

# Cleanup
# rm -rf "$TEST_DIR"

# Cross-OS test recipe (VirtualBox guest as controller)

Proves the wire protocol between a Windows host and a non-Windows controller
without a second physical machine. Measured numbers live in `docs/BENCH.md`.

## Why the controller is a separate binary

`tethercraft-probe` has no GUI, capture, store or codec dependencies, so it
cross-compiles as one static binary:

```powershell
rustup target add x86_64-unknown-linux-musl
$env:RUSTFLAGS="-C linker=rust-lld"
cargo build --release -p tethercraft-probe --target x86_64-unknown-linux-musl
```

The result is about 1 MB and needs no toolchain, glibc or runtime in the guest.
Delete the musl target directory afterwards: the build tree is large and this
machine has little free space on D:.

## Steps

1. Host on Windows, listening on all interfaces (the guest reaches the host at
   the NAT gateway, 10.0.2.2):

   ```powershell
   tethercraft host --port 21150 --password-file pw.txt --grants keyboard --fake-frames
   ```

2. Copy the probe and the password file into the guest:

   ```powershell
   VBoxManage guestcontrol ubu-2404-lts copyto .\target\x86_64-unknown-linux-musl\release\tethercraft-probe /home/user/tethercraft-probe --username user --password user
   VBoxManage guestcontrol ubu-2404-lts copyto .\pw.txt /home/user/pw.txt --username user --password user
   ```

3. Run it in the guest:

   ```sh
   chmod +x /home/user/tethercraft-probe
   /home/user/tethercraft-probe --addr 10.0.2.2:21150 --password-file /home/user/pw.txt --frames 10 --input-probe --json
   ```

4. Drop `--fake-frames` on the host for real WGC capture, and add
   `--out /home/user/live.ppm` to the probe to keep a frame. The cross-OS
   recipe can only test `raw` for now: the probe has no H.264 decoder, so if
   the host streams H.264 it says so instead of hanging.

## Reading the output

The probe prints the host id and its eight fingerprint words: compare them with
what the host printed at startup. That comparison is the current trust check.

## Known blocker, Windows guest

`win11-automated` refuses guest control for the `user` account with "the
specified user account on the guest is restricted and can't be used to logon",
even though Guest Additions 7.2.6 are running and one command succeeded right
after boot. Until someone logs into that VM interactively, nothing can be
copied in or started there. The VM was restored to its `ready-1080p-2026-10-10`
baseline and left powered off after the attempt.

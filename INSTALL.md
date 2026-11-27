# Install Interconnect

Download packages from [GitHub Releases](https://github.com/FreakyPenguin/interconnect/releases),
or [build from source](docs/development.md#build-and-checks).

## Debian

Supports Debian stable and testing on amd64.

### APT repository

Use `main` for releases or `dev` for builds from every push.

```sh
channel=main # Change to dev for development builds.
sudo install -d -m 755 /etc/apt/keyrings
sudo curl -fsS -o /etc/apt/keyrings/interconnect.asc \
  https://git.famkaufmann.info/api/packages/interconnect/debian/repository.key
sudo chmod 644 /etc/apt/keyrings/interconnect.asc
echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/interconnect.asc] https://git.famkaufmann.info/api/packages/interconnect/debian $channel main" \
  | sudo tee /etc/apt/sources.list.d/interconnect.list
sudo apt update
sudo apt install interconnect
```

Switching from `dev` to `main` may require a package downgrade.
To install a downloaded `.deb` instead:

```sh
sudo apt install ./interconnect_VERSION_amd64.deb
```

## Linux tarball

Extract `interconnect-VERSION-linux-x86_64.tar.gz` and run the `interconnect`
binary inside. Requires system graphics libraries and a Secret Service provider
such as GNOME Keyring or KWallet.

## macOS

Download and extract `Interconnect-macos-arm64.tar.gz`, move `Interconnect.app`
to Applications, and open it. Apple Silicon only; ad-hoc signed and not notarized.
Development builds are available under **Actions → macOS build**.

## Verify release downloads

Download `SHA256SUMS` alongside your package. On Linux:

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

On macOS, run `shasum -a 256 Interconnect-macos-arm64.tar.gz` and compare the
result with its entry in `SHA256SUMS`.

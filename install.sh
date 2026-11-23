#!/bin/sh
# Install grm from a GitHub Release.
#
#   curl -fsSL https://raw.githubusercontent.com/chan-labs/grm/main/install.sh | sh
#
# Environment:
#   GRM_VERSION      release tag to install, e.g. v0.1.0 (default: latest)
#   GRM_INSTALL_DIR  directory to install into (default: $HOME/.local/bin)

set -eu

REPO="chan-labs/grm"
VERSION="${GRM_VERSION:-latest}"
INSTALL_DIR="${GRM_INSTALL_DIR:-$HOME/.local/bin}"

err() {
    printf 'grm-install: error: %s\n' "$*" >&2
    exit 1
}

info() {
    printf 'grm-install: %s\n' "$*"
}

detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux)
            case "$arch" in
                x86_64 | amd64) echo "x86_64-unknown-linux-musl" ;;
                aarch64 | arm64) echo "aarch64-unknown-linux-musl" ;;
                *) err "unsupported Linux architecture: $arch (supported: x86_64, aarch64)" ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64) echo "x86_64-apple-darwin" ;;
                arm64 | aarch64) echo "aarch64-apple-darwin" ;;
                *) err "unsupported macOS architecture: $arch (supported: x86_64, arm64)" ;;
            esac
            ;;
        *) err "unsupported OS: $os (supported: Linux, macOS)" ;;
    esac
}

download() {
    # download URL DEST
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        err "need curl or wget to download grm"
    fi
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        err "need sha256sum or shasum to verify the download"
    fi
}

main() {
    command -v tar >/dev/null 2>&1 || err "need tar to unpack grm"

    target="$(detect_target)"
    asset="grm-${target}.tar.gz"

    if [ "$VERSION" = "latest" ]; then
        base="https://github.com/${REPO}/releases/latest/download"
    else
        base="https://github.com/${REPO}/releases/download/${VERSION}"
    fi

    tmp="$(mktemp -d 2>/dev/null || mktemp -d -t grm-install)"
    # shellcheck disable=SC2064 # expand $tmp now, on purpose
    trap "rm -rf '$tmp'" EXIT INT TERM

    info "downloading ${asset} (${VERSION})"
    download "${base}/${asset}" "${tmp}/${asset}" ||
        err "failed to download ${base}/${asset}"
    download "${base}/${asset}.sha256" "${tmp}/${asset}.sha256" ||
        err "failed to download ${base}/${asset}.sha256"

    expected="$(awk '{print $1; exit}' "${tmp}/${asset}.sha256")"
    actual="$(sha256_of "${tmp}/${asset}")"
    [ -n "$expected" ] || err "checksum file is empty"
    if [ "$expected" != "$actual" ]; then
        err "checksum mismatch for ${asset} (expected ${expected}, got ${actual})"
    fi
    info "checksum verified"

    tar -xzf "${tmp}/${asset}" -C "$tmp" grm ||
        err "failed to unpack ${asset}"

    mkdir -p "$INSTALL_DIR" || err "cannot create ${INSTALL_DIR}"
    # Copy to a temp name then rename, so a running grm is never half-written.
    cp "${tmp}/grm" "${INSTALL_DIR}/.grm.tmp.$$"
    chmod 755 "${INSTALL_DIR}/.grm.tmp.$$"
    mv -f "${INSTALL_DIR}/.grm.tmp.$$" "${INSTALL_DIR}/grm"
    info "installed grm to ${INSTALL_DIR}/grm"

    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            info "${INSTALL_DIR} is not on your PATH; add it with:"
            # shellcheck disable=SC2016 # print a literal $PATH for the user
            printf '\n    export PATH="%s:$PATH"\n\n' "$INSTALL_DIR"
            ;;
    esac
}

main "$@"

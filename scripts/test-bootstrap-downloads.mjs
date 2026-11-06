import assert from "node:assert/strict";
import { execFile, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import https from "node:https";
import net from "node:net";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { after, before, test } from "node:test";

const run = promisify(execFile);
const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
const openssl = process.platform === "win32" ? "C:/Program Files/Git/usr/bin/openssl.exe" : "openssl";
const payload = Buffer.from("verified FFmpeg archive fixture\n");
const digest = createHash("sha256").update(payload).digest("hex");
let root;
let cert;
let key;

before(() => {
  root = mkdtempSync(join(realpathSync(tmpdir()), "whispertube-download-test-"));
  // A config file also works with macOS LibreSSL, which may lack -addext.
  writeFileSync(join(root, "openssl.cnf"), `[req]
distinguished_name = subject
x509_extensions = extensions
prompt = no
[subject]
CN = localhost
[extensions]
subjectAltName = IP:127.0.0.1
basicConstraints = critical,CA:TRUE
`);
  const result = spawnSync(openssl, ["req", "-x509", "-newkey", "rsa:2048", "-nodes",
    "-keyout", join(root, "key.pem"), "-out", join(root, "cert.pem"), "-days", "1",
    "-config", join(root, "openssl.cnf")], {
    encoding: "utf8", timeout: 15_000, windowsHide: true,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  cert = readFileSync(join(root, "cert.pem"));
  key = readFileSync(join(root, "key.pem"));
});

after(() => {
  if (root) {
    assert.equal(dirname(realpathSync(root)), realpathSync(tmpdir()));
    rmSync(root, { recursive: true, force: true });
  }
});

function downloadBlock(platform) {
  const source = readFileSync(`scripts/setup-${platform}.sh`, "utf8");
  const start = source.indexOf("curl --fail --location", source.indexOf("ffmpeg_archive="));
  const end = source.indexOf('tar -xf "$ffmpeg_archive"', start);
  assert.ok(start >= 0 && end > start);
  return source.slice(start, end).replace(
    "https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz", "${DOWNLOAD_URL}",
  );
}

for (const platform of ["macos", "linux"]) {
  for (const scenario of ["reset-then-success", "baseline-reset", "permanent-reset", "corrupt-archive"]) {
    test(`FFmpeg ${platform} download: ${scenario}`, { timeout: 25_000 }, async () => {
      let attempts = 0;
      const sockets = new Set();
      const secure = https.createServer({ cert, key }, (_request, response) => {
        response.end(scenario === "corrupt-archive" ? Buffer.from("corrupt") : payload);
      });
      // First reset happens before TLS negotiation, reproducing curl error 35.
      // The test CA is trusted explicitly; TLS verification remains enabled.
      const server = net.createServer((socket) => {
        sockets.add(socket);
        socket.once("close", () => sockets.delete(socket));
        attempts++;
        if (scenario === "permanent-reset" || (["reset-then-success", "baseline-reset"].includes(scenario) && attempts === 1)) {
          socket.destroy();
        } else {
          secure.emit("connection", socket);
        }
      });
      await new Promise((resolve, reject) => {
        server.once("error", reject);
        server.listen(0, "127.0.0.1", resolve);
      });
      const destination = join(root, `${platform}-${scenario}.tar.xz`);
      try {
        const script = `set -euo pipefail
ffmpeg_archive="$DESTINATION"
FFMPEG_SOURCE_SHA256="$EXPECTED_HASH"
# Schannel ignores CURL_CA_BUNDLE; explicitly trust only this fixture's CA.
curl() { command curl --cacert "$TEST_CA" "$@"; }
# Git for Windows lacks shasum; use the same SHA-256 check syntax via coreutils.
if ! command -v shasum >/dev/null 2>&1; then
  shasum() { [[ "$1" == -a && "$2" == 256 ]] || return 2; shift 2; sha256sum "$@"; }
fi
${scenario === "baseline-reset" ? downloadBlock(platform).replace("--retry-all-errors", "") : downloadBlock(platform)}
`;
        let failure;
        try {
          await run(bash, ["--noprofile", "--norc", "-c", script], {
            env: { ...process.env, DOWNLOAD_URL: `https://127.0.0.1:${server.address().port}/archive`,
              DESTINATION: destination.replaceAll("\\", "/"), EXPECTED_HASH: digest,
              TEST_CA: join(root, "cert.pem").replaceAll("\\", "/") },
            encoding: "utf8", timeout: 20_000, windowsHide: true,
          });
        } catch (error) {
          failure = error;
        }
        if (scenario === "reset-then-success") {
          assert.ifError(failure);
          assert.equal(attempts, 2);
          assert.deepEqual(readFileSync(destination), payload);
        } else {
          assert.ok(failure, "failure must propagate instead of continuing to extraction");
          assert.equal(failure.killed, false, "the production retry budget must stop before the test deadline");
          assert.notEqual(failure.code, 0);
          if (scenario === "baseline-reset") {
            assert.equal(attempts, 1, "the previous curl command must fail without retrying TLS errors");
            assert.match(failure.stderr, /curl: \(35\)/);
          } else if (scenario === "permanent-reset") {
            assert.equal(attempts, 4, "one initial attempt plus three retries");
            assert.match(failure.stderr, /curl: \(35\)/);
          } else {
            assert.equal(attempts, 1);
            assert.match(failure.stderr, /Checksum source FFmpeg tidak cocok/);
          }
        }
      } finally {
        for (const socket of sockets) socket.destroy();
        await new Promise((resolve) => server.close(resolve));
        secure.close();
      }
    });
  }
}

test("Windows FFmpeg URL uses the versioned official mirror and keeps its approved digest", () => {
  const source = readFileSync("scripts/setup-windows.ps1", "utf8");
  const version = source.match(/\$FfmpegVersion = "([^"]+)"/)?.[1];
  const hash = source.match(/\$FfmpegSha256 = "([a-f0-9]+)"/)?.[1];
  const url = source.match(/Download-File "([^"]+)" \$FfmpegZip/)?.[1]
    ?.replaceAll("$FfmpegVersion", version);
  assert.equal(url, `https://github.com/GyanD/codexffmpeg/releases/download/${version}/ffmpeg-${version}-essentials_build.zip`);
  assert.equal(version, "9.0.1");
  assert.equal(hash, "fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9");
});

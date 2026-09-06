# Homebrew formula. Publish in a tap (github.com/<you>/homebrew-tap as Formula/qoral.rb):
#   brew tap <you>/tap && brew install qoral
# On each release update `url`, `sha256` (curl -sL <url> | shasum -a 256) and the bottle-free
# `resource`-less build below. Prebuilt binaries are also attached to GitHub releases.
class Qoral < Formula
  desc "Many coding agents, one terminal: they talk, debate, and document"
  homepage "https://github.com/shreeyanshujha/qoral"
  url "https://github.com/shreeyanshujha/qoral/archive/refs/tags/v0.3.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_TARBALL"
  license "MIT"
  head "https://github.com/shreeyanshujha/qoral.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  def caveats
    <<~EOS
      Alt chords need Option to send Meta:
        Terminal.app: Settings → Profiles → Keyboard → "Use Option as Meta key"
        iTerm2: Profiles → Keys → Left Option key: Esc+
      Install at least one agent CLI (claude, codex, agy, gemini), then run: qoral doctor
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/qoral --version")
  end
end

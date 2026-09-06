# Homebrew formula template. Publish it in a tap repository (e.g. github.com/<you>/homebrew-tap as Formula/qoral.rb)
# and users install with:  brew tap <you>/tap && brew install qoral
# Update url/sha256 on each release:  curl -sL <url> | shasum -a 256
class Qoral < Formula
  desc "Many coding agents, one terminal: they talk, debate, and document"
  homepage "https://github.com/shreeyanshujha/qoral"
  url "https://github.com/shreeyanshujha/qoral/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_TARBALL"
  license "MIT"

  depends_on "node"
  depends_on "tmux"

  def install
    libexec.install "bin", "lib", "package.json"
    (bin/"qoral").write_env_script libexec/"bin/qoral.js", {}
    chmod 0755, libexec/"bin/qoral.js"
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
    assert_match version.to_s, shell_output("#{bin}/qoral version")
  end
end

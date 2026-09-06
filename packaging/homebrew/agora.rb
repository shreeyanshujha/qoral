# Homebrew formula template. Publish it in a tap repository (e.g. github.com/<you>/homebrew-tap as Formula/agora.rb)
# and users install with:  brew tap <you>/tap && brew install agora
# Update url/sha256 on each release:  curl -sL <url> | shasum -a 256
class Agora < Formula
  desc "Many coding agents, one terminal: they talk, debate, and document"
  homepage "https://github.com/shreeyanshujha/agora"
  url "https://github.com/shreeyanshujha/agora/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_TARBALL"
  license "MIT"

  depends_on "node"
  depends_on "tmux"

  def install
    libexec.install "bin", "lib", "package.json"
    (bin/"agora").write_env_script libexec/"bin/agora.js", {}
    chmod 0755, libexec/"bin/agora.js"
  end

  def caveats
    <<~EOS
      Alt chords need Option to send Meta:
        Terminal.app: Settings → Profiles → Keyboard → "Use Option as Meta key"
        iTerm2: Profiles → Keys → Left Option key: Esc+
      Install at least one agent CLI (claude, codex, agy, gemini), then run: agora doctor
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/agora version")
  end
end

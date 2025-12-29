class McpReticle < Formula
  desc "Real-time debugging proxy for MCP (Model Context Protocol) servers"
  homepage "https://github.com/labterminal/reticle"
  version "0.1.0"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/labterminal/reticle/releases/download/v#{version}/reticle-cli-darwin-aarch64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_DARWIN_ARM64"
    else
      url "https://github.com/labterminal/reticle/releases/download/v#{version}/reticle-cli-darwin-x86_64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_DARWIN_X64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/labterminal/reticle/releases/download/v#{version}/reticle-cli-linux-aarch64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"
    else
      url "https://github.com/labterminal/reticle/releases/download/v#{version}/reticle-cli-linux-x86_64.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_X64"
    end
  end

  def install
    bin.install "reticle" => "mcp-reticle"
  end

  test do
    assert_match "reticle", shell_output("#{bin}/mcp-reticle --version")
  end
end

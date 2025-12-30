class McpReticle < Formula
  desc "Real-time debugging proxy for MCP (Model Context Protocol) servers"
  homepage "https://github.com/labterminal/mcp-reticle"
  version "0.1.0-rc.7"
  license "BSL-1.1"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/labterminal/mcp-reticle/releases/download/v#{version}/reticle-cli-darwin-aarch64.tar.gz"
      sha256 "63ae4d9411337595d5b82f41338f7bbef76af15b20df70797112ab6346e1a1e7"
    else
      url "https://github.com/labterminal/mcp-reticle/releases/download/v#{version}/reticle-cli-darwin-x86_64.tar.gz"
      sha256 "ada152a1574bd1ff2838ffc6f45667e5c1e26daac46f83152beb3a9e99e2cbdd"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/labterminal/mcp-reticle/releases/download/v#{version}/reticle-cli-linux-aarch64.tar.gz"
      sha256 "b6ffc8f01a39e082c05a2777976fe144d6f08a8ddd747fee897486e0c8a4e775"
    else
      url "https://github.com/labterminal/mcp-reticle/releases/download/v#{version}/reticle-cli-linux-x86_64.tar.gz"
      sha256 "4bf511100d55a52b81564cad700f238601a09c03c730dd6e656f1cd3e32c1a11"
    end
  end

  def install
    bin.install "reticle" => "mcp-reticle"
  end

  test do
    assert_match "reticle", shell_output("#{bin}/mcp-reticle --version")
  end
end

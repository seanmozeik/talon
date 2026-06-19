class Talon < Formula
  desc "Hybrid retrieval for Obsidian vaults: BM25 + semantic + reranker, with grounded answers and MCP"
  homepage "https://github.com/seanmozeik/talon"
  version "0.4.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-darwin-arm64.tar.gz"
      sha256 "c96e3b3b0a424b0602ef4f7e24c61ff781d352421b5ff13680b274cc857674c0"
    else
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-darwin-x64.tar.gz"
      sha256 "0a4133335ccea09d1eb9175ce80725e7ad9bc8245dc698350c793c6a08bc7553"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-linux-arm64.tar.gz"
      sha256 "d9817f6ade3619ffa227fc99b1b9ff0a5b2bc7e3d8fe2c78ebac574a18cde66b"
    else
      url "https://github.com/seanmozeik/talon/releases/download/v#{version}/talon-linux-x64.tar.gz"
      sha256 "35f8db8755162605ec4bf3d50d2b090f2b03eee0cb0bf361eab69696b5badad9"
    end
  end

  def install
    if OS.mac?
      binary_name = Hardware::CPU.arm? ? "talon-darwin-arm64" : "talon-darwin-x64"
    else
      binary_name = Hardware::CPU.arm? ? "talon-linux-arm64" : "talon-linux-x64"
    end
    bin.install binary_name => "talon"
  end

  test do
    assert_match "talon", shell_output("#{bin}/talon --version")
  end
end

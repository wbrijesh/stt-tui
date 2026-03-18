# stt-tui

A terminal speech-to-text interface powered by OpenAI.

Record your voice directly from the terminal and get transcriptions back. No browser, no GUI, no distractions.

## Install

### Homebrew

```bash
brew tap wbrijesh/tools
brew install stt-tui
```

### From source

```bash
git clone https://github.com/wbrijesh/stt-tui.git
cd stt-tui
cargo install --path .
```

## Setup

On first launch, you'll be prompted to enter your OpenAI API key. It gets saved to `~/.config/stt-tui/config.toml`.

You can also set it via environment variable:

```bash
export OPENAI_API_KEY=sk-...
```

## Usage

```
SPACE   Start / stop recording
h / l   Navigate between transcriptions
y       Yank (copy) current to clipboard
c       Clear all transcriptions
q/ESC   Quit
?       Help
```

## Requirements

- macOS (uses CoreAudio for mic input, pbcopy for clipboard)
- OpenAI API key with access to `gpt-4o-mini-transcribe`

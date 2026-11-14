# macOS accessibility adapter

Vendored from published `accesskit_macos` 0.26.3, package SHA-256 `ce02dc63b43f0c9296af9ac946312a2dc8814427d7a64d2d600971dac55b6076`, upstream AccessKit commit `c88605b96d04431f9c3c792464a0f2f253480e94`, `platforms/macos`. Source, normalized manifest and README retain upstream structure. License texts come from that exact upstream commit. AccessKit core and consumer remain registry dependencies.

The local correction exposes headings using AppKit `AXHeading` instead of `Heading`, and forwards positive AccessKit heading levels through the legacy `AXHeadingLevel` attribute. The harness-free native regression uses a real offscreen NSView and public Adapter on the main thread; image/link and braille attributes are positive controls. No VoiceOver setting is changed.

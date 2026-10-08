# Dioxus native experiment

I want to explore a rewrite of this app using Dioxus instead of Tauri. Importantly, I mean Dioxus with the native Blitz renderer, NOT the received Dioxus architecture with webview ("diet Electron", just like Tauri)! My dissatisfaction with Tauri's memory consumption floor necessitated by the webview architecture is the whole point of even thinking about a rewrite.

Make a thorough assessment and plan for an experimental Dioxus native rewrite of the app. Write it up in a new file here called dioxus-assessment.md

## Particular goals
1. Preserve portability across major desktop systems.
2. Make the app efficient on all fronts: speed, memory, CPU, binary. A slightly larger binary would be an acceptable price for improvements in the other aspects, especially memory.
3. Try to preserve the current UI. If not possible in all aspects, or if obvious improvements to the UI are easily available, flag this to the user. Markup support in particular is clunky in the current Tauri implementation, but this is not a priority for now.

## Resources
- Devin's assessment: https://deepwiki.com/search/can-i-build-a-pdf-reader-with_8cb65a1d-85ea-4416-bd1e-114929662ab7
- Dioxus native renderer docs: https://deepwiki.com/DioxusLabs/dioxus/5.6-native-renderer-(blitzvello)

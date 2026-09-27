// OpenWolf plugin entry — installed by `openwolf init --agent opencode`.
// Implementation lives in ./openwolf/ so it can stay multi-file; this entry
// is the only module OpenCode's plugin loader instantiates.
export { OpenWolf } from "./openwolf/index.js"

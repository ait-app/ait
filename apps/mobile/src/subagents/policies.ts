// Pure-data entry point for callers that don't want React Native deps in
// their dependency graph (e.g. workspace-tabs/agent-visibility.ts is plain
// data derivation and its tests run without an RN environment).
export { isWorkspaceRootAgent } from "./workspace-root-policy";

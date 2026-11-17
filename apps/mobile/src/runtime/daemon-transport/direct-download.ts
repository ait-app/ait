// Native uses Expo's streaming implementation instead of React Native's buffered fetch.
export const downloadFetch: typeof fetch = (...args) => fetch(...args);

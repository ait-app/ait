import { installErrorHandlers } from "./install-error-handlers";

// Install before router, stores and providers initialize, including in production builds.
installErrorHandlers();

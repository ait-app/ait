import { MutationCache, QueryCache, QueryClient } from "@tanstack/react-query";
import { appErrorReporter } from "@/diagnostics/error-reporter";

export const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error, query) =>
      appErrorReporter.report(error, {
        source: "query",
        context: { queryKey: query.queryKey },
      }),
  }),
  mutationCache: new MutationCache({
    onError: (error, _variables, _context, mutation) => {
      appErrorReporter.report(error, {
        source: "mutation",
        context: { mutationKey: mutation.options.mutationKey },
      });
    },
  }),
  defaultOptions: {
    queries: {
      staleTime: Infinity,
      refetchOnMount: false,
      refetchOnReconnect: false,
      refetchOnWindowFocus: false,
    },
  },
});

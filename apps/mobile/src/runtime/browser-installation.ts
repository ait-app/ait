const POOL_KEY = "ait.browser.installations";
const TAB_KEY = "ait.browser.installation";
const validId = (value: unknown): value is string =>
  typeof value === "string" && /^[0-9a-f-]{36}$/i.test(value);
interface InstallationLease {
  installationId: string;
  release(): void;
}

/** Reuse a bounded set of browser nodes, while duplicate tabs own different leases. */
export async function acquireBrowserInstallation(): Promise<InstallationLease> {
  const preferred = sessionStorage.getItem(TAB_KEY);
  if (!navigator.locks) {
    const installationId = validId(preferred) ? preferred : crypto.randomUUID();
    sessionStorage.setItem(TAB_KEY, installationId);
    return { installationId, release() {} };
  }
  function claim(installationId: string): Promise<InstallationLease | null> {
    return new Promise((resolve, reject) => {
      void navigator.locks
        .request(`ait.browser.node.${installationId}`, { ifAvailable: true }, async (lock) => {
          if (!lock) {
            resolve(null);
            return;
          }
          await new Promise<void>((release) => resolve({ installationId, release }));
        })
        .catch(reject);
    });
  }
  return navigator.locks.request("ait.browser.installation-pool", async () => {
    let pool: string[] = [];
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(POOL_KEY) || "[]");
      if (Array.isArray(saved)) pool = [...new Set(saved.filter(validId))].slice(0, 16);
    } catch {
      // Invalid saved metadata does not invalidate the account credential.
    }
    if (validId(preferred) && !pool.includes(preferred) && pool.length < 16) pool.push(preferred);
    const candidates = [...pool].sort((a, b) => (a === preferred ? -1 : b === preferred ? 1 : 0));
    let lease: InstallationLease | null = null;
    for (const id of candidates) {
      lease = await claim(id);
      if (lease) break;
    }
    if (!lease) {
      if (pool.length >= 16)
        throw new Error("Close another AIT client tab before opening a new one.");
      const id = crypto.randomUUID();
      lease = await claim(id);
      pool.push(id);
    }
    if (!lease) throw new Error("Could not reserve a browser node. Reload the page.");
    try {
      localStorage.setItem(POOL_KEY, JSON.stringify(pool));
      sessionStorage.setItem(TAB_KEY, lease.installationId);
      return lease;
    } catch (error) {
      lease.release();
      throw error;
    }
  });
}

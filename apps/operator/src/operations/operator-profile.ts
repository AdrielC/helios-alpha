import { onBeforeUnmount, onMounted, shallowRef } from "vue";

export interface OperatorProfile { display_name: string; role: "owner" | "operator" }

// This is display identity only. CommandPort remains the source of permissions.
export function useOperatorProfile() {
  const profile = shallowRef<OperatorProfile>({ display_name: "Operator", role: "operator" });
  let timer: ReturnType<typeof setInterval> | undefined;
  let disposed = false;
  async function refresh() {
    try {
      const response = await fetch("/api/v1/operations/operator-profile", { cache: "no-store", signal: AbortSignal.timeout(5000) });
      const value: unknown = await response.json();
      if (!response.ok || !value || typeof value !== "object") return;
      const data = value as Record<string, unknown>;
      if (typeof data.display_name !== "string" || !data.display_name.trim() || data.display_name.length > 80 || /[\u0000-\u001f\u007f]/.test(data.display_name) || !["owner", "operator"].includes(String(data.role))) return;
      if (!disposed) profile.value = { display_name: data.display_name, role: data.role as OperatorProfile["role"] };
    } catch { /* A temporary outage does not rename the local owner. */ }
  }
  onMounted(() => { void refresh(); timer = setInterval(() => { void refresh(); }, 60_000); });
  onBeforeUnmount(() => { disposed = true; clearInterval(timer); });
  return profile;
}

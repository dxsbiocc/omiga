/** Fired when the active LLM provider row changes (Settings quick-switch, save-as-default, composer switch). */
export const OMIGA_PROVIDER_CHANGED_EVENT = "omiga-provider-changed";

export function notifyProviderChanged(): void {
  window.dispatchEvent(new CustomEvent(OMIGA_PROVIDER_CHANGED_EVENT));
}

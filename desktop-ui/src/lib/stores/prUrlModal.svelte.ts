/** Shared reactive state — use exported $state object so all readers re-render. */
export const prUrlModal = $state({
  open: false,
  url: "",
  submitting: false,
});

export function isPrUrlModalOpen(): boolean {
  return prUrlModal.open;
}

export function openPrUrlModal(): void {
  prUrlModal.url = "";
  prUrlModal.submitting = false;
  prUrlModal.open = true;
}

export function closePrUrlModal(): void {
  prUrlModal.open = false;
  prUrlModal.url = "";
  prUrlModal.submitting = false;
}

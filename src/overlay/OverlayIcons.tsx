export function CopyIcon({
  confirmed,
  failed,
}: {
  confirmed: boolean;
  failed: boolean;
}) {
  if (confirmed) {
    return (
      <svg viewBox="0 0 20 20" aria-hidden="true">
        <path d="m4.5 10.2 3.3 3.3 7.7-8" />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <rect x="7" y="7" width="9" height="9" rx="2" />
      <path d="M13 7V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2" />
      {failed && <path className="error-mark" d="M15.5 3.5v3m0 2v.1" />}
    </svg>
  );
}

export function RetryIcon() {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="M15.7 6.2V2.8m0 0h-3.4m3.4 0-2.3 2.3A6.2 6.2 0 1 0 16 11" />
    </svg>
  );
}

export function ExplanationIcon() {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="M3.5 6.5V5.2a1.7 1.7 0 0 1 1.7-1.7h1.3" />
      <path d="M13.5 3.5h1.3a1.7 1.7 0 0 1 1.7 1.7v1.3" />
      <path d="M16.5 13.5v1.3a1.7 1.7 0 0 1-1.7 1.7h-1.3" />
      <path d="M6.5 16.5H5.2a1.7 1.7 0 0 1-1.7-1.7v-1.3" />
      <path d="M6.5 7.5h7m-7 2.7h7m-7 2.7h4.5" />
    </svg>
  );
}

export function CloseIcon() {
  return (
    <svg viewBox="0 0 20 20" aria-hidden="true">
      <path d="m5.5 5.5 9 9m0-9-9 9" />
    </svg>
  );
}

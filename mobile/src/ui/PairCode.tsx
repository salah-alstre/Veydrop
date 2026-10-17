/** A pairing code exactly as the engine generated it (ASCII digits, leading zeros kept). */
export function PairCode({ code, label, className = "" }: { code: string; label?: string; className?: string }) {
  return (
    <span className={`pair-code ${className}`} dir="ltr" lang="en" aria-label={label} data-testid="pair-code">
      {code}
    </span>
  );
}

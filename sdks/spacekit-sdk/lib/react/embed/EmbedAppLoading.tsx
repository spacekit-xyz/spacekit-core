import { type CSSProperties, type FC } from "react";

export interface EmbedAppLoadingProps {
  embedded?: boolean;
  label?: string;
}

const EmbedAppLoading: FC<EmbedAppLoadingProps> = ({
  embedded = false,
  label = "Loading app…",
}) => {
  const font = embedded ? '"Hanken Grotesk", sans-serif' : "'DM Sans', sans-serif";

  const wrap: CSSProperties = {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    justifyContent: "center",
    gap: 14,
  };

  // Solid ink — avoid background-clip:text (WebView can paint the gradient as a flat bar).
  const labelStyle: CSSProperties = {
    fontSize: 14,
    fontFamily: font,
    fontWeight: 600,
    letterSpacing: "-0.01em",
    color: embedded ? "#9aa3b5" : "#94a3b8",
  };

  return (
    <div role="status" aria-live="polite" aria-busy="true" style={wrap}>
      <svg width="28" height="28" viewBox="0 0 28 28" aria-hidden style={{ display: "block" }}>
        <circle
          cx="14"
          cy="14"
          r="11"
          fill="none"
          stroke="rgba(34, 211, 238, 0.14)"
          strokeWidth="2.5"
        />
        <path
          d="M14 3a11 11 0 0 1 11 11"
          fill="none"
          stroke="#22d3ee"
          strokeWidth="2.5"
          strokeLinecap="round"
        >
          <animateTransform
            attributeName="transform"
            type="rotate"
            from="0 14 14"
            to="360 14 14"
            dur="0.75s"
            repeatCount="indefinite"
          />
        </path>
      </svg>
      <span style={labelStyle}>{label}</span>
    </div>
  );
};

export default EmbedAppLoading;

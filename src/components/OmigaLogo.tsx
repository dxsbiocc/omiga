/**
 * OmigaLogo - app-internal capybara mark.
 *
 * Uses the same capybara artwork as the packaged app icon, with a slightly
 * larger visual scale so small in-app placements remain readable.
 */

import omigaLogoUrl from "../assets/omiga-logo.svg";

interface OmigaLogoProps {
  size?: number;
  animated?: boolean;
  style?: React.CSSProperties;
  className?: string;
}

export function OmigaLogo({
  size = 40,
  animated = true,
  style,
  className,
}: OmigaLogoProps) {
  const transform = style?.transform
    ? `${style.transform} scale(1.12)`
    : "scale(1.12)";

  return (
    <img
      src={omigaLogoUrl}
      width={size}
      height={size}
      alt="Omiga"
      className={className}
      data-animated={animated ? "true" : "false"}
      style={{
        ...style,
        display: "block",
        objectFit: "contain",
        transform,
        transformOrigin: "center",
      }}
    />
  );
}

// Icon.jsx — tiny wrapper. Uses the SVGs in assets/icons/ via <img>.
// Inherits color via CSS filter trick when needed; for our use, we mostly
// just consume the colored variants in the components themselves.
function Icon({ name, size = 16, color, style, ...rest }) {
  const filter = color ? colorToFilter(color) : undefined;
  return (
    <img
      src={`../../assets/icons/${name}.svg`}
      width={size}
      height={size}
      alt=""
      style={{ display: 'inline-block', verticalAlign: 'middle', filter, ...style }}
      {...rest}
    />
  );
}

// Crude SVG-color-via-filter map. We only need a few colors.
function colorToFilter(c) {
  switch (c) {
    case 'ink':       return 'brightness(0) saturate(100%) invert(11%) sepia(7%) saturate(840%) hue-rotate(118deg) brightness(96%) contrast(94%)';
    case 'ink-2':     return 'brightness(0) saturate(100%) invert(35%) sepia(7%) saturate(345%) hue-rotate(118deg) brightness(95%) contrast(91%)';
    case 'ink-3':     return 'brightness(0) saturate(100%) invert(63%) sepia(5%) saturate(385%) hue-rotate(118deg) brightness(95%) contrast(89%)';
    case 'sage-deep': return 'brightness(0) saturate(100%) invert(48%) sepia(20%) saturate(540%) hue-rotate(85deg) brightness(94%) contrast(85%)';
    case 'moss':      return 'brightness(0) saturate(100%) invert(22%) sepia(20%) saturate(700%) hue-rotate(85deg) brightness(85%) contrast(85%)';
    case 'white':     return 'brightness(0) invert(1)';
    case 'danger':    return 'brightness(0) saturate(100%) invert(40%) sepia(40%) saturate(580%) hue-rotate(338deg) brightness(94%) contrast(85%)';
    default: return undefined;
  }
}

window.Icon = Icon;

import light from '../assets/logo-light.png';
import dark from '../assets/logo-dark.png';

/** The Notercrux app icon. Shows the light or dark variant to match the active theme (see app.css). */
export function AppLogo({ size = 40 }: { size?: number }) {
  return (
    <span className="app-logo" style={{ width: size, height: size }}>
      <img className="app-logo-light" src={light} alt="Notercrux" width={size} height={size} />
      <img
        className="app-logo-dark"
        src={dark}
        alt=""
        aria-hidden="true"
        width={size}
        height={size}
      />
    </span>
  );
}

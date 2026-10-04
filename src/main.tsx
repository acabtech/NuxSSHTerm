import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// NOTE: StrictMode is intentionally omitted — its double-mount would spawn two
// PTY/SSH sessions per tab in development.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
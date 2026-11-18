// The shared styles load first, so a page's own stylesheet wins any tie with them.
import "./styles.css";
import { render } from "preact";
import { App } from "./app";
import { startMotion } from "./ui/motion";

const root = document.getElementById("app");
if (root) {
  startMotion(root);
  render(<App />, root);
}

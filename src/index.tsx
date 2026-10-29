import { render } from "solid-js/web";
import App from "./App";
import "highlight.js/styles/github-dark.css";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  render(() => <App />, root);
}

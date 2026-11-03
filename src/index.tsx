import { render } from "solid-js/web";
import App from "./App";
import QuickAsk from "./components/QuickAsk";
import "highlight.js/styles/github-dark.css";
import "./styles.css";

const root = document.getElementById("root");
const overlay = new URLSearchParams(window.location.search).has("overlay");
if (root) {
  render(() => (overlay ? <QuickAsk /> : <App />), root);
}

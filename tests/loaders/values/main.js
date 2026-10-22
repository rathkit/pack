// Imported but never used: the copied file must not be written.
import unusedImage from './unused.png';
import fs from "node:fs";
import path from "node:path";
import logo from "./logo.png";
import note from "./note.txt";
import blob64 from "./blob.dat";
import skipped from "./skip.dat2";
import dataUrl from "./data.url";
import bin from "./bin.bin";

const here = process.env.FIXTURE_DIR;
const out = process.env.OUT_DIR;
console.log("text", JSON.stringify(note) === JSON.stringify(fs.readFileSync(path.join(here, "note.txt"), "utf8")));
console.log("base64", blob64 === fs.readFileSync(path.join(here, "blob.dat")).toString("base64"));
console.log("empty", JSON.stringify(skipped));
console.log("file url", /^\.\/logo-[a-z2-7]{8}\.png$/.test(logo));
console.log("file copied", fs.readFileSync(path.join(out, logo)).equals(fs.readFileSync(path.join(here, "logo.png"))));
console.log("dataurl", dataUrl === "data:application/octet-stream;base64," + fs.readFileSync(path.join(here, "data.url")).toString("base64"));
console.log("binary", bin instanceof Uint8Array, Array.from(bin).join(","));

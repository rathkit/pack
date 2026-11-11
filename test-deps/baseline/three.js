import { WebGLRenderer, Scene, PerspectiveCamera, Mesh, BoxGeometry, MeshBasicMaterial } from 'three';
const r = new WebGLRenderer(); const s = new Scene(); const c = new PerspectiveCamera();
s.add(new Mesh(new BoxGeometry(), new MeshBasicMaterial()));
console.log(typeof r.render, s.children.length, c.fov);

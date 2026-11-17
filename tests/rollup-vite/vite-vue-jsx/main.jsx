import { defineComponent } from 'vue';
const C = defineComponent({ render: () => <div>hi</div> });
console.log(typeof C.render);

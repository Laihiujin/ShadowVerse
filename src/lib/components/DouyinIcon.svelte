<script lang="ts">
  export let className: string = "";
  export { className as class };
</script>

<img
  src="/imgs/douyin.svg"
  alt="avatar"
  class={"w-8 h-8 rounded-full " + className}
/>

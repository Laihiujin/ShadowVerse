<script>
  import { createEventDispatcher } from "svelte";

  const dispatch = createEventDispatcher();

  // activeUrl is shared between pages.
  export let activeUrl = "overview";
  export let label = "";
  export let value = "";
  export let dot = false;

  $: routeValue = value || label;
</script>

<button
  on:click={() => dispatch("activeChange", routeValue)}
  class="flex w-full items-center space-x-2 px-3 py-2 rounded-lg {activeUrl ===
  routeValue
    ? 'bg-blue-500/10 text-[#0A84FF] dark:bg-transparent dark:text-white'
    : 'text-gray-700 dark:text-white'} hover:bg-[#e5e5e5] dark:hover:bg-transparent"
>
  <slot
    name="icon"
    class={activeUrl === routeValue
      ? "text-[#0A84FF] dark:text-white"
      : "text-gray-700 dark:text-white"}
  ></slot>
  <span>{label}</span>
  {#if dot}
    <div class="absolute right-6 w-2 h-2 bg-red-500 rounded-full"></div>
  {/if}
</button>

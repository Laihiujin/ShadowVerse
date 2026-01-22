<script lang="ts">
  let className = "";
  export { className as class };
</script>

<img src="/imgs/Tiktok.svg" class={`tiktok-light ${className}`} alt="TikTok" />
<img
  src="/imgs/TikTok-Wordmark.svg"
  class={`tiktok-dark ${className}`}
  alt="TikTok"
/>

<style>
  .tiktok-dark {
    display: none;
  }

  :global(.dark) .tiktok-light {
    display: none;
  }

  :global(.dark) .tiktok-dark {
    display: inline-block;
  }
</style>

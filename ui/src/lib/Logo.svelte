<script lang="ts">
  import source from "../assets/brand/sakatsu.svg";
  let { size = "3rem" }: { size?: string } = $props();
</script>

<img src={source} alt="" width="1000" height="1000" style:width={size} style:height={size} />

<style>
  img {
    display: block;
    flex: none;
    border-radius: 18%;
  }
</style>

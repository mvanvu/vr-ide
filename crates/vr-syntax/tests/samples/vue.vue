<!-- A counter component. -->
<template>
  <button class="counter" @click="count++">Count: {{ count }}</button>
</template>

<script setup lang="ts">
import { ref } from "vue";
const count = ref(0);
</script>

<style scoped>
.counter { color: red; }
</style>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { shaderSearch, shaderTagOptions } from "../../store/community/categorySearch";
import CommunitySearchBar from "./CommunitySearchBar.vue";

const { t } = useI18n();
</script>

<template>
  <CommunitySearchBar
    :search="shaderSearch"
    :title="t('download.search.title', { category: t('download.categories.shader') })"
    show-tag
    :tag-options="shaderTagOptions"
    id-prefix="sh"
  />
</template>

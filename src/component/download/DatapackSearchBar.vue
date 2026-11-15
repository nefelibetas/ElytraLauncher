<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { datapackSearch, datapackTagOptions } from "../../store/community/categorySearch";
import CommunitySearchBar from "./CommunitySearchBar.vue";

const { t } = useI18n();
</script>

<template>
  <CommunitySearchBar
    :search="datapackSearch"
    :title="t('download.search.title', { category: t('download.categories.datapack') })"
    show-tag
    :tag-options="datapackTagOptions"
    id-prefix="da"
  />
</template>

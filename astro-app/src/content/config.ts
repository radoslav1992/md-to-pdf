import { defineCollection, z } from 'astro:content';

const blog = defineCollection({
  type: 'content',
  schema: z.object({
    title: z.string(),
    description: z.string(),
    publishDate: z.coerce.date(),
    updatedDate: z.coerce.date().optional(),
    author: z.string().default('Universal Converter Team'),
    tags: z.array(z.string()).default([]),
    draft: z.boolean().default(false),
  }),
});

const legal = defineCollection({
  type: 'content',
  schema: z.object({
    title: z.string(),
    description: z.string(),
    effectiveDate: z.coerce.date(),
    version: z.string().default('1.0'),
  }),
});

export const collections = {
  blog,
  legal,
};

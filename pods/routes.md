# Podcast RSS feed
`GET /user/<user ID>/podcasts/<ID>`
```json
{
    "name": "this american life",
    "description": "a podcast about american lives",
    "rss": "link/to/rss/feed",
}
```

`PUT /user/<user ID>/podcasts/`
```json
{
    "rss": "<uri>"
}
```
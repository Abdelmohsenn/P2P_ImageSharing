import requests
import os

# Pexels API key
PEXELS_API_KEY = 'SOME_API_KEY'

# Folder to save the images
SAVE_FOLDER = 'pexels_images'
os.makedirs(SAVE_FOLDER, exist_ok=True)

# Pexels API endpoint for image search
PEXELS_URL = 'https://api.pexels.com/v1/search'

# Search query and the number of images to download
# Can be changed to any query like 'cars', 'city', etc.
query = 'nature'

# Maximum allowed by Pexels per request
per_page = 80

# Total number of images to download
total_images = 10000

# API headers with the authorization key
headers = {
    'Authorization': PEXELS_API_KEY
}

def download_image(url, save_path):
    """Downloads an image from the provided URL and saves it to the specified path."""
    try:
        img_data = requests.get(url).content
        with open(save_path, 'wb') as handler:
            handler.write(img_data)
        print(f"Downloaded: {save_path}")
    except Exception as e:
        print(f"Failed to download {url}: {e}")

def main():
    """Retrieves and downloads images based on the query using the Pexels API."""
    downloaded = 0
    page = 1

    while downloaded < total_images:
        print(f"Fetching page {page}...")

        # Request image metadata via an API request
        response = requests.get(PEXELS_URL, headers=headers, params={
            'query': query,
            'per_page': per_page,
            'page': page
        })

        if response.status_code != 200:
            print(f"Error: {response.status_code}")
            break

        data = response.json()

        if 'photos' not in data or not data['photos']:
            print('No more images found.')
            break

        # Iterate over the obtained images and download them if not already present
        for photo in data['photos']:
            # The original high-res image URL
            img_url = photo['src']['original']

            # Image ID for file naming
            img_id = photo['id']
            img_name = os.path.join(SAVE_FOLDER, f"{img_id}.jpg")

            # Check if the image already exists before downloading
            if not os.path.exists(img_name):
                download_image(img_url, img_name)
                downloaded += 1
            else:
                print(f"Image {img_name} already exists, skipping download.")

            # Once the target number of images is reached, break from the loop
            if downloaded >= total_images:
                break

        # Page number is incremented for the next request
        page += 1

    print(f"Downloaded {downloaded} images to {SAVE_FOLDER}.")

# Run the image download process
if __name__ == '__main__':
    main()



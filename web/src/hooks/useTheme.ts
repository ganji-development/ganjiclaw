import { useContext } from 'react';
import { ThemeContext } from '../contexts/ThemeContextUtils';

export const useTheme = () => useContext(ThemeContext);
